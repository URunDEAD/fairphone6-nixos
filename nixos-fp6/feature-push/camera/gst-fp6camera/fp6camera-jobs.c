/* SPDX-License-Identifier: LGPL-2.1-or-later
 *
 * fp6camera PHASE 2: capture jobs. #included by fp6camera-phase2.c.
 *
 * A job takes frames from the processed stream (the same probe point as
 * the long-exposure stacks: before the zoom crop and the stream flip) and
 * processes them in a background thread at low CPU priority, so the
 * viewfinder keeps running; progress and the result path are properties
 * the app polls.
 *
 *   "job-start" (kind, path, rotate, quality, params) -> gboolean
 *       kind    hdr | ...
 *       path    output file (JPEG)
 *       rotate  clockwise display rotation (as for stack-start)
 *       params  GstStructure string with options, e.g. "p, ev=2.0"
 *   "job-stop"   finish an open-ended job now
 *   "job-cancel" abandon it (or acknowledge a finished one)
 *
 * Bracketed kinds (hdr) lock white balance and focus at the values the
 * automatic loops had when the job started, then step the exposure from
 * the auto exposure's exposure x gain (fp6m override, like a stack) and
 * take one frame per step once the frame metadata confirm the values.
 */

#include "fp6-imaging.h"
#include "fp6-vision.h"

#include <sys/resource.h>
#include <sys/syscall.h>

typedef enum
{
  JOB_IDLE = 0,
  JOB_CAPTURE = 1,
  JOB_PROCESS = 2,
  JOB_DONE = 3,
  JOB_ERROR = 4,
} Fp6JobState;

typedef enum
{
  KIND_NONE = 0,
  KIND_HDR,
  KIND_NIGHT,
  KIND_TIMELAPSE,
  KIND_BRACKET,
  KIND_PANORAMA,
} Fp6JobKind;

static const struct
{
  const gchar *name;
  Fp6JobKind kind;
} fp6j_kinds[] = {
  {"hdr", KIND_HDR},
  {"night", KIND_NIGHT},
  {"timelapse", KIND_TIMELAPSE},
  {"bracket", KIND_BRACKET},
  {"panorama", KIND_PANORAMA},
};

typedef struct
{
  guint8 *data;
  GstVideoInfo info;
  gint64 exp_us;
  gdouble gain;
  gdouble ev;
} Fp6Frame;

#define FP6J_MAX_STEPS 32
/* exposure above which handheld frames blur; brackets use gain instead */
#define FP6J_HANDHELD_US 66000
#define FP6J_MAX_GAIN 16.0
/* night mode: longest handheld frame of the burst */
#define FP6J_NIGHT_US 100000

static void
fp6j_frame_free (gpointer p)
{
  Fp6Frame *f = p;
  g_free (f->data);
  g_free (f);
}

/* ------------------------------------------------------------------------ */
/* exposure plan                                                            */

/* exposure time and gain for ev stops above the auto exposure e0 x g0 */
static void
fp6j_exposure_for_ev (gint64 e0, gdouble g0, gdouble ev, gint64 e_cap,
    gint64 e_max, gint64 * e, gdouble * g)
{
  gdouble total = (gdouble) e0 * g0 * pow (2.0, ev);
  gdouble ee, gg;

  if (ev <= 0) {
    /* darker: drop the gain first (less noise), then the time */
    gg = MAX (1.0, g0 * pow (2.0, ev));
    ee = total / gg;
  } else {
    /* brighter: longer up to the handheld limit, then gain */
    ee = MIN (total / g0, (gdouble) MAX (e_cap, e0));
    gg = MIN (total / ee, MAX (FP6J_MAX_GAIN, g0));
  }
  *e = CLAMP ((gint64) llround (ee), 40, MAX (e_max, 40));
  *g = MAX (gg, 1.0);
}

/* lock WB/focus and set the step's exposure. p->lock held. */
static void
fp6j_apply_step (Fp6LibcameraSrc * self, gint step)
{
  Fp6Phase2 *p = self->p2;
  Fp6Manual *m = self->manual;

  g_mutex_lock (&m->lock);
  m->ovr = TRUE;
  m->ovr_exp_us = p->step_exp[step];
  m->ovr_gain = p->step_gain[step];
  m->ovr_gr = p->base_gr;
  m->ovr_gb = p->base_gb;
  m->ovr_lens = p->base_lens;
  fp6m_apply_locked (m);
  g_mutex_unlock (&m->lock);
  p->settle_ok = 0;
  p->settle_deadline = g_get_monotonic_time () + 2 * G_USEC_PER_SEC +
      4 * p->step_exp[step];
}

/* back to the user's (or automatic) settings. p->lock held. */
static void
fp6j_release_camera (Fp6LibcameraSrc * self)
{
  Fp6Manual *m = self->manual;
  g_mutex_lock (&m->lock);
  if (m->ovr) {
    m->ovr = FALSE;
    fp6m_apply_locked (m);
  }
  g_mutex_unlock (&m->lock);
}

/* ------------------------------------------------------------------------ */
/* worker                                                                   */

static void
fp6j_progress_cb (gpointer user, gdouble f)
{
  Fp6LibcameraSrc *self = user;
  g_mutex_lock (&self->p2->lock);
  self->p2->job_progress = f;
  g_mutex_unlock (&self->p2->lock);
}

static Fp6Image *
fp6j_frame_image (const Fp6Frame * f)
{
  const GstVideoFormatInfo *fi = f->info.finfo;
  return fp6_image_from_packed (f->data, GST_VIDEO_INFO_PLANE_STRIDE (&f->info,
          0), GST_VIDEO_INFO_WIDTH (&f->info), GST_VIDEO_INFO_HEIGHT (&f->info),
      GST_VIDEO_INFO_COMP_PSTRIDE (&f->info, 0),
      GST_VIDEO_FORMAT_INFO_POFFSET (fi, GST_VIDEO_COMP_R),
      GST_VIDEO_FORMAT_INFO_POFFSET (fi, GST_VIDEO_COMP_G),
      GST_VIDEO_FORMAT_INFO_POFFSET (fi, GST_VIDEO_COMP_B));
}

/* the digital zoom of the viewfinder (centre crop) */
static Fp6Image *
fp6j_apply_zoom (Fp6Image * img, gfloat zoom)
{
  gint cw, ch;
  Fp6Image *c;

  if (zoom <= 1.01f || !img)
    return img;
  cw = ((gint) (img->w / zoom)) & ~1;
  ch = ((gint) ((gint64) cw * img->h / img->w)) & ~1;
  c = fp6_image_crop (img, (img->w - cw) / 2, (img->h - ch) / 2, cw, ch);
  if (!c)
    return img;
  fp6_image_free (img);
  return c;
}

static gboolean
fp6j_save (Fp6LibcameraSrc * self, Fp6Image * img, const gchar * path,
    gint rotate, const gchar * flip, gint quality, gint64 exp_us,
    gdouble gain, const gchar * software, GError ** err)
{
  Fp6SaveJob job = { 0 };
  gchar *dir = g_path_get_dirname (path);

  g_mkdir_with_parents (dir, 0755);
  g_free (dir);
  job.src = self;
  gst_video_info_set_format (&job.info, GST_VIDEO_FORMAT_RGB, img->w, img->h);
  if (GST_VIDEO_INFO_PLANE_STRIDE (&job.info, 0) != img->stride) {
    g_set_error (err, GST_CORE_ERROR, GST_CORE_ERROR_FAILED, "stride");
    return FALSE;
  }
  job.image = img->data;
  job.path = (gchar *) path;
  job.orientation = rotate;
  job.quality = quality;
  job.iso = gain > 0 ? (gint) lround (gain * 100) : 0;
  job.exposure_s = exp_us / 1e6;
  job.flip = flip && g_strcmp0 (flip, "none") ? (gchar *) flip : NULL;
  job.software = software;
  return fp6m_save_jpeg (&job, err);
}

static Fp6Image *
fp6j_process_hdr (Fp6LibcameraSrc * self, GPtrArray * frames,
    const Fp6Progress * prog, GError ** err)
{
  guint n = frames->len;
  Fp6Image *imgs[16] = { NULL }, *out = NULL;
  gint dx[16] = { 0 }, dy[16] = { 0 };
  gint64 t0 = g_get_monotonic_time ();

  if (n < 2 || n > 16) {
    g_set_error (err, GST_CORE_ERROR, GST_CORE_ERROR_FAILED,
        "HDR needs 2..16 frames, have %u", n);
    return NULL;
  }
  for (guint i = 0; i < n; i++) {
    imgs[i] = fp6j_frame_image (g_ptr_array_index (frames, i));
    if (!imgs[i])
      goto oom;
  }
  /* align everything to the EV 0 frame (index 0); exposure invariant */
  for (guint i = 1; i < n; i++) {
    Fp6Frame *f0 = g_ptr_array_index (frames, 0), *fi =
        g_ptr_array_index (frames, i);
    /* the real exposure ratio of the frames (metadata) */
    gdouble ev = log2 ((fi->exp_us * fi->gain) / MAX (1.0,
            f0->exp_us * f0->gain));
    fp6_align_mtb_ev (imgs[0], imgs[i], ev, 64, &dx[i], &dy[i]);
    GST_INFO_OBJECT (self, "HDR frame %u (EV %+.1f) offset %d,%d", i,
        ((Fp6Frame *) g_ptr_array_index (frames, i))->ev, dx[i], dy[i]);
  }
  fp6_progress (prog, 0.1);
  {
    Fp6Progress sub = fp6_progress_sub (prog, 0.1, 0.9);
    out = fp6_mertens (imgs, dx, dy, n, &sub);
  }
  if (!out)
    goto oom;
  GST_INFO_OBJECT (self, "HDR fused %u frames in %.2f s", n,
      (g_get_monotonic_time () - t0) / 1e6);
  for (guint i = 0; i < n; i++)
    fp6_image_free (imgs[i]);
  return out;
oom:
  for (guint i = 0; i < n; i++)
    fp6_image_free (imgs[i]);
  if (!fp6_cancelled (prog))
    g_set_error (err, GST_CORE_ERROR, GST_CORE_ERROR_FAILED, "out of memory");
  return NULL;
}

static Fp6Image *
fp6j_process_night (Fp6LibcameraSrc * self, GPtrArray * frames,
    const Fp6Progress * prog, gint * ref, GError ** err)
{
  guint n = frames->len;
  Fp6Image **imgs = g_new0 (Fp6Image *, n), *out = NULL;
  gint64 t0 = g_get_monotonic_time ();

  for (guint i = 0; i < n; i++) {
    Fp6Frame *f = g_ptr_array_index (frames, i);
    imgs[i] = fp6j_frame_image (f);
    /* the packed copy is no longer needed: keep the peak memory down */
    g_clear_pointer (&f->data, g_free);
    if (!imgs[i])
      goto out;
  }
  fp6_progress (prog, 0.05);
  {
    Fp6Progress sub = fp6_progress_sub (prog, 0.05, 0.95);
    out = fp6_night_merge (imgs, n, 1.0, ref, &sub);
  }
  GST_INFO_OBJECT (self, "night: merged %u frames (reference %d) in %.2f s",
      n, *ref, (g_get_monotonic_time () - t0) / 1e6);
out:
  for (guint i = 0; i < n; i++)
    fp6_image_free (imgs[i]);
  g_free (imgs);
  if (!out && !fp6_cancelled (prog))
    g_set_error (err, GST_CORE_ERROR, GST_CORE_ERROR_FAILED, "out of memory");
  return out;
}

/* ------------------------------------------------------------------------ */
/* timelapse: frames at an interval -> H.264 MP4, encoded while capturing  */

static void fp6j_finish_capture (Fp6LibcameraSrc * self);
static void fp6j_fail_locked (Fp6LibcameraSrc * self, const gchar * msg);

/* videoflip method for an EXIF orientation */
static const gchar *
fp6j_flip_method (guint16 exif)
{
  static const gchar *const m[9] = { "none", "none", "horizontal-flip",
    "rotate-180", "vertical-flip", "upper-left-diagonal", "clockwise",
    "upper-right-diagonal", "counterclockwise"
  };
  return m[exif <= 8 ? exif : 0];
}

/* p->lock held. Build and start the encoder for frames of p->job_info. */
static gboolean
fp6j_timelapse_open (Fp6LibcameraSrc * self, GError ** err)
{
  Fp6Phase2 *p = self->p2;
  GstElement *pipe, *src, *flip, *conv, *capsf, *enc, *parse, *mux, *sink;
  guint16 o = fp6_dng_orientation (p->flip, p->job_rotate);
  gint w = GST_VIDEO_INFO_WIDTH (&p->job_info);
  gint h = GST_VIDEO_INFO_HEIGHT (&p->job_info), ow, oh;
  gchar *tmp;
  GstCaps *caps;

  /* encode size: long side tl_size (even), stream aspect */
  ow = MIN (p->tl_size, w) & ~1;
  oh = ((gint) ((gint64) ow * h / w)) & ~1;
  if (o >= 5) {                 /* rotated by 90: portrait video */
    gint t = ow;
    ow = oh;
    oh = t;
  }
  pipe = gst_pipeline_new ("fp6-timelapse");
  src = gst_element_factory_make ("appsrc", NULL);
  flip = gst_element_factory_make ("videoflip", NULL);
  conv = gst_element_factory_make ("videoconvertscale", NULL);
  capsf = gst_element_factory_make ("capsfilter", NULL);
  enc = gst_element_factory_make ("x264enc", NULL);
  parse = gst_element_factory_make ("h264parse", NULL);
  mux = gst_element_factory_make ("mp4mux", NULL);
  sink = gst_element_factory_make ("filesink", NULL);
  if (!src || !flip || !conv || !capsf || !enc || !parse || !mux || !sink) {
    g_set_error (err, GST_CORE_ERROR, GST_CORE_ERROR_MISSING_PLUGIN,
        "timelapse encoder elements missing (x264enc/h264parse/mp4mux)");
    gst_object_unref (pipe);
    g_clear_object (&src);
    g_clear_object (&flip);
    g_clear_object (&conv);
    g_clear_object (&capsf);
    g_clear_object (&enc);
    g_clear_object (&parse);
    g_clear_object (&mux);
    g_clear_object (&sink);
    return FALSE;
  }
  caps = gst_video_info_to_caps (&p->job_info);
  gst_caps_set_simple (caps, "framerate", GST_TYPE_FRACTION, p->tl_fps, 1,
      NULL);
  g_object_set (src, "caps", caps, "format", GST_FORMAT_TIME, "block", FALSE,
      "max-bytes", (guint64) 512 * 1024 * 1024, NULL);
  gst_caps_unref (caps);
  gst_util_set_object_arg (G_OBJECT (flip), "method", fp6j_flip_method (o));
  caps = gst_caps_new_simple ("video/x-raw", "format", G_TYPE_STRING, "I420",
      "width", G_TYPE_INT, ow, "height", G_TYPE_INT, oh, NULL);
  g_object_set (capsf, "caps", caps, NULL);
  gst_caps_unref (caps);
  /* constant quality; few threads so the viewfinder keeps its CPU */
  gst_util_set_object_arg (G_OBJECT (enc), "pass", "quant");
  gst_util_set_object_arg (G_OBJECT (enc), "speed-preset", "faster");
  g_object_set (enc, "quantizer", 21, "threads", 2, "key-int-max",
      p->tl_fps * 2, NULL);
  tmp = g_strdup_printf ("%s.part", p->job_path);
  g_object_set (sink, "location", tmp, NULL);
  g_free (tmp);
  gst_bin_add_many (GST_BIN (pipe), src, flip, conv, capsf, enc, parse, mux,
      sink, NULL);
  if (!gst_element_link_many (src, flip, conv, capsf, enc, parse, mux, sink,
          NULL)
      || gst_element_set_state (pipe,
          GST_STATE_PLAYING) == GST_STATE_CHANGE_FAILURE) {
    g_set_error (err, GST_CORE_ERROR, GST_CORE_ERROR_NEGOTIATION,
        "cannot start the timelapse encoder");
    gst_element_set_state (pipe, GST_STATE_NULL);
    gst_object_unref (pipe);
    return FALSE;
  }
  p->tl_pipe = pipe;
  p->tl_src = src;
  GST_INFO_OBJECT (self, "timelapse: %dx%d -> %dx%d (%s) at %d fps, every "
      "%.2f s -> %s", w, h, ow, oh, fp6j_flip_method (o), p->tl_fps,
      p->tl_interval_us / 1e6, p->job_path);
  return TRUE;
}

/* streaming thread, p->lock held */
static void
fp6j_timelapse_frame (Fp6LibcameraSrc * self, GstBuffer * buf, gint64 now)
{
  Fp6Phase2 *p = self->p2;
  GstVideoFrame vf;
  GstBuffer *copy;
  GstFlowReturn fr;

  if (p->job_stop || (p->job_target > 0 && p->job_frames >= p->job_target)) {
    fp6j_finish_capture (self);
    return;
  }
  if (!p->tl_pipe) {
    GError *err = NULL;
    if (!fp6j_timelapse_open (self, &err)) {
      fp6j_fail_locked (self, err ? err->message : "encoder");
      g_clear_error (&err);
      return;
    }
    p->tl_next = now;
  }
  if (now < p->tl_next)
    return;
  p->tl_next += p->tl_interval_us;
  if (p->tl_next < now)         /* fell behind (slow frames): no burst */
    p->tl_next = now + p->tl_interval_us;

  /* tightly packed copy (the camera buffer goes back to its pool) */
  if (!gst_video_frame_map (&vf, &p->job_info, buf, GST_MAP_READ))
    return;
  copy = gst_buffer_new_allocate (NULL, GST_VIDEO_INFO_SIZE (&p->job_info),
      NULL);
  if (copy) {
    GstMapInfo m;
    gst_buffer_map (copy, &m, GST_MAP_WRITE);
    for (gint y = 0; y < GST_VIDEO_INFO_HEIGHT (&p->job_info); y++)
      memcpy (m.data + (gsize) y * GST_VIDEO_INFO_PLANE_STRIDE (&p->job_info,
              0), (guint8 *) GST_VIDEO_FRAME_PLANE_DATA (&vf, 0) +
          (gsize) y * GST_VIDEO_FRAME_PLANE_STRIDE (&vf, 0),
          MIN (GST_VIDEO_INFO_PLANE_STRIDE (&p->job_info, 0),
              GST_VIDEO_FRAME_PLANE_STRIDE (&vf, 0)));
    gst_buffer_unmap (copy, &m);
    GST_BUFFER_PTS (copy) = gst_util_uint64_scale (p->job_frames, GST_SECOND,
        p->tl_fps);
    GST_BUFFER_DURATION (copy) = gst_util_uint64_scale (1, GST_SECOND,
        p->tl_fps);
    g_signal_emit_by_name (p->tl_src, "push-buffer", copy, &fr);
    gst_buffer_unref (copy);
    p->job_frames++;
  }
  gst_video_frame_unmap (&vf);
}

/* worker: finish the MP4 */
static gboolean
fp6j_timelapse_close (Fp6LibcameraSrc * self, GstElement * pipe,
    GstElement * src, const gchar * path, gint frames, GError ** err)
{
  GstFlowReturn fr;
  GstBus *bus = gst_element_get_bus (pipe);
  GstMessage *msg;
  gchar *tmp = g_strdup_printf ("%s.part", path);
  gboolean ok = FALSE;

  g_signal_emit_by_name (src, "end-of-stream", &fr);
  msg = gst_bus_timed_pop_filtered (bus, 120 * GST_SECOND,
      GST_MESSAGE_EOS | GST_MESSAGE_ERROR);
  if (msg && GST_MESSAGE_TYPE (msg) == GST_MESSAGE_EOS)
    ok = frames > 0;
  else if (msg)
    gst_message_parse_error (msg, err, NULL);
  else
    g_set_error (err, GST_CORE_ERROR, GST_CORE_ERROR_FAILED,
        "timelapse encoder timed out");
  if (msg)
    gst_message_unref (msg);
  gst_object_unref (bus);
  gst_element_set_state (pipe, GST_STATE_NULL);
  if (ok && g_rename (tmp, path) != 0) {
    g_set_error (err, G_FILE_ERROR, g_file_error_from_errno (errno),
        "cannot rename %s", tmp);
    ok = FALSE;
  }
  if (!ok) {
    g_unlink (tmp);
    if (frames == 0 && err && !*err)
      g_set_error (err, GST_CORE_ERROR, GST_CORE_ERROR_FAILED,
          "no frames captured");
  }
  g_free (tmp);
  return ok;
}

static gpointer
fp6j_worker (gpointer data)
{
  Fp6LibcameraSrc *self = data;
  Fp6Phase2 *p = self->p2;
  Fp6Progress prog = { fp6j_progress_cb, self, 0, 1, &p->job_cancel };
  GPtrArray *frames;
  Fp6JobKind kind;
  gchar *path, *flip, *result = NULL;
  gint rotate, quality;
  gfloat zoom;
  Fp6Image *img = NULL;
  GError *err = NULL;
  Fp6Frame *ref;
  GstElement *tl_pipe, *tl_src;
  gint tl_frames;
  Fp6vPano *pano = NULL;

  /* below the viewfinder's threads (inherited by the processing threads) */
  setpriority (PRIO_PROCESS, (id_t) syscall (SYS_gettid), 10);

  g_mutex_lock (&p->lock);
  frames = p->frames;
  p->frames = NULL;
  kind = p->job_kind;
  path = g_strdup (p->job_path);
  flip = g_strdup (p->flip);
  rotate = p->job_rotate;
  quality = p->job_quality;
  zoom = p->job_zoom;
  tl_pipe = p->tl_pipe;
  tl_src = p->tl_src;
  tl_frames = p->job_frames;
  p->tl_pipe = p->tl_src = NULL;
  g_mutex_unlock (&p->lock);

  ref = frames && frames->len ? g_ptr_array_index (frames, 0) : NULL;
  switch (kind) {
    case KIND_HDR:
      img = fp6j_process_hdr (self, frames, &prog, &err);
      if (img) {
        img = fp6j_apply_zoom (img, zoom);
        if (fp6j_save (self, img, path, rotate, flip, quality, ref->exp_us,
                ref->gain, "lomiri-camera-app (fp6 HDR)", &err))
          result = g_strdup (path);
      }
      break;
    case KIND_NIGHT:{
      gint r = 0;
      img = fp6j_process_night (self, frames, &prog, &r, &err);
      if (img) {
        img = fp6j_apply_zoom (img, zoom);
        if (fp6j_save (self, img, path, rotate, flip, quality, ref->exp_us,
                ref->gain, "lomiri-camera-app (fp6 night)", &err))
          result = g_strdup (path);
      }
      break;
    }
    case KIND_BRACKET:{
      /* every frame as its own JPEG: <path without .jpg>_EV+2.0.jpg */
      gchar *base = g_str_has_suffix (path, ".jpg") ?
          g_strndup (path, strlen (path) - 4) : g_strdup (path);
      GString *list = g_string_new (NULL);
      for (guint i = 0; i < frames->len && !p->job_cancel; i++) {
        Fp6Frame *f = g_ptr_array_index (frames, i);
        gchar *fp = g_strdup_printf ("%s_EV%+.1f.jpg", base, f->ev);
        Fp6Image *one = fp6j_apply_zoom (fp6j_frame_image (f), zoom);
        if (!one || !fp6j_save (self, one, fp, rotate, flip, quality,
                f->exp_us, f->gain, "lomiri-camera-app (fp6 bracket)", &err)) {
          fp6_image_free (one);
          g_free (fp);
          if (!err)
            g_set_error (&err, GST_CORE_ERROR, GST_CORE_ERROR_FAILED,
                "out of memory");
          break;
        }
        fp6_image_free (one);
        g_string_append_printf (list, "%s%s", list->len ? "\n" : "", fp);
        g_free (fp);
        fp6_progress (&prog, (i + 1.0) / frames->len);
      }
      if (!err && list->len)
        result = g_string_free (list, FALSE);
      else
        g_string_free (list, TRUE);
      g_free (base);
      break;
    }
    case KIND_PANORAMA:{
      /* Rust (fp6-vision): upright output, saved without rotation */
      Fp6vImage *in = g_new0 (Fp6vImage, frames->len), out = { 0 };
      gint rc = -1;
      for (guint i = 0; i < frames->len; i++) {
        Fp6Frame *f = g_ptr_array_index (frames, i);
        const GstVideoFormatInfo *fi = f->info.finfo;
        in[i].w = GST_VIDEO_INFO_WIDTH (&f->info);
        in[i].h = GST_VIDEO_INFO_HEIGHT (&f->info);
        in[i].stride = GST_VIDEO_INFO_PLANE_STRIDE (&f->info, 0);
        in[i].ps = GST_VIDEO_INFO_COMP_PSTRIDE (&f->info, 0);
        in[i].ro = GST_VIDEO_FORMAT_INFO_POFFSET (fi, GST_VIDEO_COMP_R);
        in[i].go = GST_VIDEO_FORMAT_INFO_POFFSET (fi, GST_VIDEO_COMP_G);
        in[i].bo = GST_VIDEO_FORMAT_INFO_POFFSET (fi, GST_VIDEO_COMP_B);
        in[i].data = f->data;
      }
      if (kind == KIND_PANORAMA) {
        g_mutex_lock (&p->lock);
        pano = p->pano;
        p->pano = NULL;
        g_mutex_unlock (&p->lock);
        rc = pano && frames->len >= 2 ? fp6v_pano_stitch (pano, in,
            frames->len, &out, fp6j_progress_cb, self) : -3;
        if (rc != 0)
          g_set_error (&err, GST_CORE_ERROR, GST_CORE_ERROR_FAILED, "%s",
              rc == -3 ? "sweep too short (need 2 frames)" :
              "panorama frames do not overlap");
      }
      if (rc >= 0) {
        Fp6Image o = { out.w, out.h, out.stride, out.data };
        if (fp6j_save (self, &o, path, 0, NULL, quality, ref->exp_us,
                ref->gain, "lomiri-camera-app (fp6 panorama)", &err))
          result = g_strdup (path);
      }
      fp6v_free (&out);
      g_free (in);
      break;
    }
    case KIND_TIMELAPSE:
      if (tl_pipe && fp6j_timelapse_close (self, tl_pipe, tl_src, path,
              tl_frames, &err))
        result = g_strdup (path);
      else if (!tl_pipe && !err)
        g_set_error (&err, GST_CORE_ERROR, GST_CORE_ERROR_FAILED,
            "no frames captured");
      break;
    default:
      g_set_error (&err, GST_CORE_ERROR, GST_CORE_ERROR_FAILED,
          "unsupported job");
      break;
  }
  fp6_image_free (img);
  if (frames)
    g_ptr_array_unref (frames);
  if (tl_pipe)
    gst_object_unref (tl_pipe);
  if (pano)
    fp6v_pano_free (pano);

  g_mutex_lock (&p->lock);
  if (p->job_cancel) {
    p->job_state = JOB_IDLE;
    if (result)
      g_unlink (result);
  } else if (result) {
    g_free (p->job_result);
    p->job_result = result;
    result = NULL;
    p->job_progress = 1.0;
    p->job_state = JOB_DONE;
    GST_INFO_OBJECT (self, "job saved: %s", p->job_result);
  } else {
    g_free (p->job_error);
    p->job_error = g_strdup (err ? err->message : "processing failed");
    p->job_state = JOB_ERROR;
    GST_WARNING_OBJECT (self, "job failed: %s", p->job_error);
  }
  g_mutex_unlock (&p->lock);
  g_free (result);
  g_clear_error (&err);
  g_free (path);
  g_free (flip);
  gst_object_unref (self);
  return NULL;
}

/* p->lock held: all frames are in, hand them to the worker */
static void
fp6j_finish_capture (Fp6LibcameraSrc * self)
{
  Fp6Phase2 *p = self->p2;
  fp6j_release_camera (self);
  p->job_state = JOB_PROCESS;
  p->job_progress = 0;
  g_thread_unref (g_thread_new ("fp6-job", fp6j_worker,
          gst_object_ref (self)));
}

/* p->lock held: drop an unfinished timelapse file */
static void
fp6j_drop_timelapse (Fp6Phase2 * p)
{
  if (p->tl_pipe) {
    gchar *tmp = g_strdup_printf ("%s.part", p->job_path);
    gst_element_set_state (p->tl_pipe, GST_STATE_NULL);
    gst_object_unref (p->tl_pipe);
    p->tl_pipe = p->tl_src = NULL;
    g_unlink (tmp);
    g_free (tmp);
  }
}

static void
fp6j_fail_locked (Fp6LibcameraSrc * self, const gchar * msg)
{
  Fp6Phase2 *p = self->p2;
  fp6j_drop_timelapse (p);
  fp6j_release_camera (self);
  g_clear_pointer (&p->frames, g_ptr_array_unref);
  g_free (p->job_error);
  p->job_error = g_strdup (msg);
  p->job_state = JOB_ERROR;
  GST_WARNING_OBJECT (self, "job: %s", msg);
}

/* ------------------------------------------------------------------------ */
/* streaming thread                                                         */

static Fp6Frame *
fp6j_copy_frame (Fp6Phase2 * p, GstBuffer * buf)
{
  GstVideoFrame vf;
  Fp6Frame *f;

  if (!gst_video_frame_map (&vf, &p->job_info, buf, GST_MAP_READ))
    return NULL;
  f = g_new0 (Fp6Frame, 1);
  f->info = p->job_info;
  /* tightly packed copy in the frame's own layout */
  f->data = g_try_malloc (GST_VIDEO_INFO_SIZE (&p->job_info));
  if (f->data) {
    const guint8 *s = GST_VIDEO_FRAME_PLANE_DATA (&vf, 0);
    gint ss = GST_VIDEO_FRAME_PLANE_STRIDE (&vf, 0);
    gint ds = GST_VIDEO_INFO_PLANE_STRIDE (&p->job_info, 0);
    for (gint y = 0; y < GST_VIDEO_INFO_HEIGHT (&p->job_info); y++)
      memcpy (f->data + (gsize) y * ds, s + (gsize) y * ss, MIN (ss, ds));
  }
  gst_video_frame_unmap (&vf);
  if (!f->data) {
    g_free (f);
    return NULL;
  }
  return f;
}

/* bracket/burst kinds: one frame per step once the exposure is confirmed */
static void
fp6j_step_frame (Fp6LibcameraSrc * self, GstBuffer * buf, gint64 now)
{
  Fp6Phase2 *p = self->p2;
  Fp6Manual *m = self->manual;
  gint64 e, want_e = p->step_exp[p->step];
  gdouble g, want_g = p->step_gain[p->step];
  gboolean ok;
  Fp6Frame *f;

  g_mutex_lock (&m->lock);
  e = fp6m_get_int (m->cam, "exposure-time");
  g = fp6m_get_float (m->cam, "analogue-gain");
  g_mutex_unlock (&m->lock);
  ok = e > 0 && llabs (e - want_e) <= MAX (want_e / 33, 60)
      && fabs (g - want_g) <= 0.05 * want_g + 0.05;
  p->settle_ok = ok ? p->settle_ok + 1 : 0;
  if (p->settle_ok < 2 && now < p->settle_deadline)
    return;
  if (!ok)
    GST_WARNING_OBJECT (self, "job step %d: exposure %" G_GINT64_FORMAT
        " us / gain %.2f not confirmed (have %" G_GINT64_FORMAT " / %.2f)",
        p->step, want_e, want_g, e, g);

  f = fp6j_copy_frame (p, buf);
  if (!f) {
    fp6j_fail_locked (self, "out of memory");
    return;
  }
  f->exp_us = e > 0 ? e : want_e;
  f->gain = g > 0 ? g : want_g;
  f->ev = p->step_ev[p->step];
  g_ptr_array_add (p->frames, f);
  p->job_frames++;
  GST_INFO_OBJECT (self, "job frame %d/%d: EV %+.1f, %" G_GINT64_FORMAT
      " us, gain %.2f", p->job_frames, p->n_steps, f->ev, f->exp_us, f->gain);

  p->step++;
  if (p->step >= p->n_steps) {
    fp6j_finish_capture (self);
  } else if (p->step_exp[p->step] != p->step_exp[p->step - 1]
      || p->step_gain[p->step] != p->step_gain[p->step - 1]) {
    fp6j_apply_step (self, p->step);
  }
}

/* panorama: track the sweep on a small luma image, keep keyframes */
static void
fp6j_pano_frame (Fp6LibcameraSrc * self, GstBuffer * buf, gint64 now)
{
  Fp6Phase2 *p = self->p2;
  GstVideoFrame vf;
  const GstVideoFormatInfo *fi = p->job_info.finfo;
  gint w = GST_VIDEO_INFO_WIDTH (&p->job_info) / 8;
  gint h = GST_VIDEO_INFO_HEIGHT (&p->job_info) / 8, rc;
  gint ro = GST_VIDEO_FORMAT_INFO_POFFSET (fi, GST_VIDEO_COMP_R);
  gint go = GST_VIDEO_FORMAT_INFO_POFFSET (fi, GST_VIDEO_COMP_G);
  gint bo = GST_VIDEO_FORMAT_INFO_POFFSET (fi, GST_VIDEO_COMP_B);
  gint ps = GST_VIDEO_INFO_COMP_PSTRIDE (&p->job_info, 0);
  guint8 *small;

  if (p->job_stop) {
    if (p->frames->len >= 2)
      fp6j_finish_capture (self);
    else
      fp6j_fail_locked (self, "sweep too short (need 2 frames)");
    return;
  }
  if (!p->pano) {
    guint16 o = fp6_dng_orientation (p->flip, p->job_rotate);
    p->pano = fp6v_pano_new (o, p->pano_focal, 8.0, p->pano_max);
  }
  if (!gst_video_frame_map (&vf, &p->job_info, buf, GST_MAP_READ))
    return;
  /* 1/8 size luma (sampled, every 8th pixel of every 8th row) */
  small = g_malloc ((gsize) w * h);
  for (gint y = 0; y < h; y++) {
    const guint8 *r = (const guint8 *) GST_VIDEO_FRAME_PLANE_DATA (&vf, 0) +
        (gsize) (y * 8 + 4) * GST_VIDEO_FRAME_PLANE_STRIDE (&vf, 0);
    for (gint x = 0; x < w; x++) {
      const guint8 *q = r + (gsize) (x * 8 + 4) * ps;
      small[(gsize) y * w + x] = (q[ro] * 77 + q[go] * 150 + q[bo] * 29) >> 8;
    }
  }
  gst_video_frame_unmap (&vf);
  rc = fp6v_pano_feed (p->pano, small, w, h, w);
  g_free (small);
  fp6v_pano_status (p->pano, &p->pano_step, &p->pano_drift, &p->pano_dir);
  if (rc >= 1) {
    Fp6Frame *f = fp6j_copy_frame (p, buf);
    if (!f) {
      fp6j_fail_locked (self, "out of memory");
      return;
    }
    f->exp_us = p->step_exp[0];
    f->gain = p->step_gain[0];
    g_ptr_array_add (p->frames, f);
    p->job_frames = p->frames->len;
    GST_INFO_OBJECT (self, "panorama keyframe %u (drift %.2f)",
        p->frames->len, p->pano_drift);
  }
  if (rc == 2)
    fp6j_finish_capture (self);
}

/* crop sink pad: every processed frame */
static GstPadProbeReturn
fp6j_probe (GstPad * pad, GstPadProbeInfo * pi, gpointer user)
{
  Fp6LibcameraSrc *self = user;
  Fp6Phase2 *p = self->p2;
  GstBuffer *buf = GST_PAD_PROBE_INFO_BUFFER (pi);
  gint64 now;

  if (G_LIKELY (p->job_state != JOB_CAPTURE))   /* unlocked peek */
    return GST_PAD_PROBE_OK;
  now = g_get_monotonic_time ();
  g_mutex_lock (&p->lock);
  if (p->job_state != JOB_CAPTURE) {
    g_mutex_unlock (&p->lock);
    return GST_PAD_PROBE_OK;
  }
  if (p->job_cancel) {
    fp6j_drop_timelapse (p);
    fp6j_release_camera (self);
    g_clear_pointer (&p->frames, g_ptr_array_unref);
    p->job_state = JOB_IDLE;
    g_mutex_unlock (&p->lock);
    return GST_PAD_PROBE_OK;
  }
  if (!p->job_info_ok) {
    GstCaps *caps = gst_pad_get_current_caps (pad);
    p->job_info_ok = caps && gst_video_info_from_caps (&p->job_info, caps)
        && GST_VIDEO_INFO_N_PLANES (&p->job_info) == 1
        && GST_VIDEO_INFO_COMP_PSTRIDE (&p->job_info, 0) >= 3
        && GST_VIDEO_FORMAT_INFO_IS_RGB (p->job_info.finfo);
    if (caps)
      gst_caps_unref (caps);
    if (!p->job_info_ok) {
      fp6j_fail_locked (self, "unsupported stream format");
      g_mutex_unlock (&p->lock);
      return GST_PAD_PROBE_OK;
    }
  }
  if (now > p->job_deadline) {
    fp6j_fail_locked (self, "capture timed out");
    g_mutex_unlock (&p->lock);
    return GST_PAD_PROBE_OK;
  }

  switch (p->job_kind) {
    case KIND_HDR:
    case KIND_NIGHT:
    case KIND_BRACKET:
      fp6j_step_frame (self, buf, now);
      break;
    case KIND_TIMELAPSE:
      fp6j_timelapse_frame (self, buf, now);
      break;
    case KIND_PANORAMA:
      fp6j_pano_frame (self, buf, now);
      break;
    default:
      break;
  }
  g_mutex_unlock (&p->lock);
  return GST_PAD_PROBE_OK;
}

/* ------------------------------------------------------------------------ */
/* signals                                                                  */

static gdouble
fp6j_param_double (const GstStructure * s, const gchar * name, gdouble def)
{
  gdouble d;
  gint i;
  if (s && gst_structure_get_double (s, name, &d))
    return d;
  if (s && gst_structure_get_int (s, name, &i))
    return i;
  return def;
}

static gboolean
fp6j_start (Fp6LibcameraSrc * self, const gchar * kind_name,
    const gchar * path, gint rotate, gint quality, const gchar * params,
    gpointer unused)
{
  Fp6Phase2 *p = self->p2;
  Fp6Manual *m = self->manual;
  Fp6JobKind kind = KIND_NONE;
  GstStructure *s = NULL;
  gint64 e0, e_max;
  gdouble g0;
  gboolean busy;

  for (guint i = 0; i < G_N_ELEMENTS (fp6j_kinds); i++)
    if (!g_strcmp0 (kind_name, fp6j_kinds[i].name))
      kind = fp6j_kinds[i].kind;
  if (kind == KIND_NONE || !path || !*path) {
    GST_WARNING_OBJECT (self, "job-start: bad kind '%s' or path",
        GST_STR_NULL (kind_name));
    return FALSE;
  }
  if (params && *params) {
    s = gst_structure_from_string (params, NULL);
    if (!s)
      GST_WARNING_OBJECT (self, "job-start: cannot parse params '%s'", params);
  }

  g_mutex_lock (&p->lock);
  g_mutex_lock (&m->lock);
  busy = !m->cam || fp6m_forced (m) || m->st == STK_SAVING
      || p->job_state == JOB_CAPTURE || p->job_state == JOB_PROCESS;
  e0 = fp6m_get_int (m->cam, "exposure-time");
  g0 = fp6m_get_float (m->cam, "analogue-gain");
  p->base_gr = fp6m_get_array (m->cam, "colour-gains", 0, 0);
  p->base_gb = fp6m_get_array (m->cam, "colour-gains", 1, 0);
  p->base_lens = m->has_focus ? fp6m_get_float (m->cam, "lens-position") : -1;
  if (m->focus >= 0 && m->has_focus)
    p->base_lens = m->focus * 100.0f;   /* the user's manual focus */
  e_max = m->cam ? fp6m_max_exposure_locked (m) : 33333;
  g_mutex_unlock (&m->lock);
  if (busy || e0 <= 0 || g0 <= 0) {
    g_mutex_unlock (&p->lock);
    if (s)
      gst_structure_free (s);
    GST_WARNING_OBJECT (self, "job-start refused (busy or no exposure yet)");
    return FALSE;
  }

  g_free (p->job_path);
  p->job_path = g_strdup (path);
  p->job_kind = kind;
  p->job_rotate = rotate;
  p->job_quality = quality > 0 ? quality : 92;
  GST_OBJECT_LOCK (self);
  p->job_zoom = self->zoom;
  GST_OBJECT_UNLOCK (self);
  g_clear_pointer (&p->job_result, g_free);
  g_clear_pointer (&p->job_error, g_free);
  g_clear_pointer (&p->frames, g_ptr_array_unref);
  p->frames = g_ptr_array_new_with_free_func (fp6j_frame_free);
  p->job_frames = 0;
  p->job_progress = 0;
  p->job_cancel = 0;
  p->job_stop = FALSE;
  p->job_info_ok = FALSE;
  p->step = 0;
  p->n_steps = 0;
  p->job_deadline = g_get_monotonic_time () + 30 * G_USEC_PER_SEC;

  switch (kind) {
    case KIND_HDR:
    case KIND_BRACKET:{
      /* EV 0 first (the moment of the tap), then darker and brighter */
      gdouble ev = CLAMP (fp6j_param_double (s, "ev", 2.0), 0.3, 4.0);
      gint frames = (gint) fp6j_param_double (s, "frames", 3);
      gdouble evs[5] = { 0, -ev, ev, -2 * ev, 2 * ev };
      frames = frames >= 5 ? 5 : 3;
      if (frames == 5)          /* 5 frames: ev is the spacing */
        evs[3] = -2 * ev, evs[4] = 2 * ev;
      for (gint i = 0; i < frames; i++) {
        p->step_ev[i] = evs[i];
        if (kind == KIND_BRACKET) {
          /* Pro bracketing: the shutter time moves (the ISO stays, as a
           * tripod user expects); gain only where the time hits a limit */
          gdouble want = e0 * pow (2.0, evs[i]);
          gint64 e = CLAMP ((gint64) llround (want), 40, MAX (e_max, 40));
          p->step_exp[i] = e;
          p->step_gain[i] = CLAMP (g0 * want / e, 1.0, 64.0);
        } else {
          fp6j_exposure_for_ev (e0, g0, evs[i], FP6J_HANDHELD_US, e_max,
              &p->step_exp[i], &p->step_gain[i]);
        }
      }
      p->n_steps = frames;
      break;
    }
    case KIND_NIGHT:{
      /* a burst of moderately long frames (handheld), a little brighter
       * than the auto exposure: the merge removes the extra noise */
      gint frames = CLAMP ((gint) fp6j_param_double (s, "frames",
              g0 >= 8 ? 10 : 8), 2, FP6J_MAX_STEPS);
      gdouble boost = CLAMP (fp6j_param_double (s, "boost", 0.5), 0, 2);
      gint64 cap = (gint64) fp6j_param_double (s, "max-exposure-us",
          FP6J_NIGHT_US);
      gdouble total = (gdouble) e0 * g0 * pow (2.0, boost);
      gint64 e = CLAMP ((gint64) MIN (total, (gdouble) MAX (cap, e0)), 40,
          e_max);
      gdouble g = CLAMP (total / e, 1.0, MAX (g0, 1.0));
      for (gint i = 0; i < frames; i++) {
        p->step_ev[i] = boost;
        p->step_exp[i] = e;
        p->step_gain[i] = g;
      }
      p->n_steps = frames;
      break;
    }
    case KIND_PANORAMA:{
      /* exposure, white balance and focus fixed for the whole sweep (seams) */
      static const gdouble focal[] = { 2400, 2840, 1500, 2300 };
      p->step_exp[0] = e0;
      p->step_gain[0] = g0;
      p->step_ev[0] = 0;
      p->n_steps = 0;
      p->pano_focal = fp6j_param_double (s, "focal", focal[p->sensor]);
      p->pano_max = CLAMP ((gint) fp6j_param_double (s, "frames", 12), 2, 24);
      p->pano_step = p->pano_drift = 0;
      p->pano_dir = 0;
      if (p->pano) {
        fp6v_pano_free (p->pano);
        p->pano = NULL;
      }
      p->job_deadline = g_get_monotonic_time () + 180 * G_USEC_PER_SEC;
      break;
    }
    case KIND_TIMELAPSE:
      /* automatic exposure throughout; no frames kept in memory */
      p->tl_interval_us = (gint64) (CLAMP (fp6j_param_double (s, "interval",
                  2.0), 0.1, 3600.0) * G_USEC_PER_SEC);
      p->tl_fps = CLAMP ((gint) fp6j_param_double (s, "fps", 30), 1, 60);
      p->tl_size = CLAMP ((gint) fp6j_param_double (s, "size", 1440), 320,
          4096);
      p->job_deadline = G_MAXINT64;
      break;
    default:
      break;
  }
  p->job_target = kind == KIND_TIMELAPSE ?
      (gint) fp6j_param_double (s, "frames", 0) : p->n_steps;
  p->job_state = JOB_CAPTURE;
  GST_INFO_OBJECT (self, "job %s: %d steps from %" G_GINT64_FORMAT " us x "
      "%.2f, WB %.3f/%.3f, lens %.1f -> %s", kind_name, p->n_steps, e0, g0,
      p->base_gr, p->base_gb, p->base_lens, path);
  if (p->n_steps > 0 || kind == KIND_PANORAMA)
    fp6j_apply_step (self, 0);
  g_mutex_unlock (&p->lock);
  if (s)
    gst_structure_free (s);
  return TRUE;
}

static void
fp6j_stop (Fp6LibcameraSrc * self, gpointer unused)
{
  Fp6Phase2 *p = self->p2;
  g_mutex_lock (&p->lock);
  if (p->job_state == JOB_CAPTURE)
    p->job_stop = TRUE;
  g_mutex_unlock (&p->lock);
}

static void
fp6j_cancel (Fp6LibcameraSrc * self, gpointer unused)
{
  Fp6Phase2 *p = self->p2;
  g_mutex_lock (&p->lock);
  if (p->job_state == JOB_CAPTURE || p->job_state == JOB_PROCESS)
    p->job_cancel = 1;
  else if (p->job_state == JOB_DONE || p->job_state == JOB_ERROR)
    p->job_state = JOB_IDLE;
  g_mutex_unlock (&p->lock);
}

/* chain torn down while capturing */
static void
fp6j_clear (Fp6LibcameraSrc * self)
{
  Fp6Phase2 *p = self->p2;
  if (p->job_state == JOB_CAPTURE) {
    if (p->job_kind == KIND_TIMELAPSE && p->tl_pipe) {
      /* keep what was recorded */
      fp6j_finish_capture (self);
      return;
    }
    fp6j_release_camera (self);
    g_clear_pointer (&p->frames, g_ptr_array_unref);
    g_free (p->job_error);
    p->job_error = g_strdup ("camera stopped");
    p->job_state = JOB_ERROR;
  }
}

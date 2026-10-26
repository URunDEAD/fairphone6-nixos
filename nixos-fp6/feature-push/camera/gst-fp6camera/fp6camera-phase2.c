/* SPDX-License-Identifier: LGPL-2.1-or-later
 *
 * fp6camera: PHASE 2 section (computational and standard features of
 * lomiri-camera-app on the FP6). #included by fp6camera.c after the
 * MANUAL CONTROLS section; it adds properties, signals and probes to
 * fp6libcamerasrc and never changes the element's normal behaviour.
 *
 * Parts (see results/feature-push-20261024/camera/phase2/README.md):
 *   fp6camera-raw.c   RAW/DNG: optional second libcamerasrc pad with the
 *                     sensor's packed Bayer stream; "raw-capture" writes the
 *                     next raw frame as DNG in a background thread.
 *   fp6camera-jobs.c  capture jobs on the processed stream (HDR ...),
 *                     processed by fp6-imaging.c in a background thread.
 */

#include "fp6-dng.h"

#define FP6P2_PROP_BASE 200

enum
{
  FP6P2_PROP_RAW = FP6P2_PROP_BASE,
  FP6P2_PROP_RAW_ACTIVE,
  FP6P2_PROP_RAW_FORMAT,
  FP6P2_PROP_RAW_STATE,
  FP6P2_PROP_RAW_RESULT,
  FP6P2_PROP_RAW_ERROR,
  FP6P2_PROP_SENSOR,
  FP6P2_PROP_JOB_STATE,
  FP6P2_PROP_JOB_KIND,
  FP6P2_PROP_JOB_PROGRESS,
  FP6P2_PROP_JOB_FRAMES,
  FP6P2_PROP_JOB_TARGET_FRAMES,
  FP6P2_PROP_JOB_RESULT,
  FP6P2_PROP_JOB_ERROR,
  FP6P2_PROP_LAST
};

typedef enum
{
  FP6_SENSOR_UNKNOWN = 0,
  FP6_SENSOR_IMX896,            /* main */
  FP6_SENSOR_OV13B10,           /* ultrawide */
  FP6_SENSOR_S5KKD1,            /* front */
} Fp6Sensor;

static const gchar *const fp6_sensor_names[] = {
  "unknown", "imx896", "ov13b10", "s5kkd1"
};

/* raw-state */
typedef enum
{
  RAW_IDLE = 0,
  RAW_ARMED = 1,                /* waiting for the next raw frame */
  RAW_SAVING = 2,
  RAW_DONE = 3,
  RAW_ERROR = 4,
} Fp6RawState;

typedef struct _Fp6Phase2
{
  Fp6LibcameraSrc *src;         /* owner, not referenced */
  GMutex lock;

  Fp6Sensor sensor;             /* of the built chain */
  gboolean swap_rb;             /* entry relabels R/B (old front driver) */
  gchar *flip;                  /* entry videoflip method */

  /* RAW */
  gboolean raw_want;            /* property, applies at the next build */
  gboolean raw_built;
  gchar *raw_format;            /* caps format, e.g. "bggr10p" */
  gint raw_w, raw_h, raw_stride;
  Fp6RawState raw_state;
  gchar *raw_path;
  gint raw_rotate;
  gint64 raw_armed_at;
  gchar *raw_result, *raw_error;

  /* capture jobs (fp6camera-jobs.c) */
  gint job_state;               /* Fp6JobState */
  gint job_kind;                /* Fp6JobKind */
  gchar *job_path;
  gint job_rotate, job_quality;
  gfloat job_zoom;
  gdouble job_progress;
  gint job_frames, job_target;
  gchar *job_result, *job_error;
  volatile gint job_cancel;
  gboolean job_stop;
  gint64 job_deadline;
  GstVideoInfo job_info;
  gboolean job_info_ok;
  GPtrArray *frames;            /* Fp6Frame */
  /* exposure plan */
  gint n_steps, step, settle_ok;
  gint64 settle_deadline;
  gint64 step_exp[32];
  gdouble step_gain[32], step_ev[32];
  gdouble base_gr, base_gb;
  gfloat base_lens;
  /* timelapse */
  GstElement *tl_pipe, *tl_src;
  gint64 tl_next, tl_interval_us;
  gint tl_fps, tl_size;
} Fp6Phase2;

static gpointer fp6p2_dng_thread (gpointer data);

/* ------------------------------------------------------------------------ */
/* sensors                                                                  */

static Fp6Sensor
fp6p2_sensor_of (const gchar * camera_name)
{
  if (!camera_name)
    return FP6_SENSOR_UNKNOWN;
  if (strstr (camera_name, "camera@1a"))
    return FP6_SENSOR_IMX896;
  if (strstr (camera_name, "camera@36"))
    return FP6_SENSOR_OV13B10;
  if (strstr (camera_name, "camera@3d"))
    return FP6_SENSOR_S5KKD1;
  return FP6_SENSOR_UNKNOWN;
}

#include "fp6camera-raw.c"
#include "fp6camera-jobs.c"

/* ------------------------------------------------------------------------ */
/* element hooks                                                            */

static Fp6Phase2 *
fp6p2_new (Fp6LibcameraSrc * self)
{
  Fp6Phase2 *p = g_new0 (Fp6Phase2, 1);
  p->src = self;
  g_mutex_init (&p->lock);
  return p;
}

static void
fp6p2_free (Fp6LibcameraSrc * self)
{
  Fp6Phase2 *p = self->p2;
  if (!p)
    return;
  g_free (p->flip);
  g_free (p->raw_format);
  g_free (p->raw_path);
  g_free (p->raw_result);
  g_free (p->raw_error);
  g_free (p->job_path);
  g_free (p->job_result);
  g_free (p->job_error);
  g_clear_pointer (&p->frames, g_ptr_array_unref);
  g_mutex_clear (&p->lock);
  g_free (p);
  self->p2 = NULL;
}

/* lsrc_build hook, after the main chain is linked (the bin holds cam/crop) */
static void
fp6p2_build (Fp6LibcameraSrc * self, GstElement * cam, GstElement * crop,
    const gchar * camera_name, const Fp6CamEntry * e, gint idx)
{
  Fp6Phase2 *p = self->p2;
  gboolean raw;

  g_mutex_lock (&p->lock);
  p->sensor = fp6p2_sensor_of (camera_name);
  p->swap_rb = idx >= 0 && e->swap_rb;
  g_free (p->flip);
  p->flip = g_strdup (idx >= 0 && e->flip ? e->flip : "none");
  raw = p->raw_want;
  p->raw_built = FALSE;
  g_clear_pointer (&p->raw_format, g_free);
  p->raw_w = p->raw_h = p->raw_stride = 0;
  if (p->raw_state == RAW_ARMED)
    p->raw_state = RAW_IDLE;
  p->job_info_ok = FALSE;
  g_mutex_unlock (&p->lock);

  {
    GstPad *pad = gst_element_get_static_pad (crop, "sink");
    gst_pad_add_probe (pad, GST_PAD_PROBE_TYPE_BUFFER, fp6j_probe, self, NULL);
    gst_object_unref (pad);
  }
  if (raw)
    fp6r_build (self, cam);
}

static void
fp6p2_clear (Fp6LibcameraSrc * self)
{
  Fp6Phase2 *p = self->p2;

  g_mutex_lock (&p->lock);
  p->raw_built = FALSE;
  if (p->raw_state == RAW_ARMED) {
    p->raw_state = RAW_ERROR;
    g_free (p->raw_error);
    p->raw_error = g_strdup ("camera stopped");
  }
  fp6j_clear (self);
  g_mutex_unlock (&p->lock);
}

static gboolean
fp6p2_set_property (Fp6LibcameraSrc * self, guint id, const GValue * value)
{
  Fp6Phase2 *p = self->p2;

  if (id < FP6P2_PROP_BASE || id >= FP6P2_PROP_LAST)
    return FALSE;
  g_mutex_lock (&p->lock);
  switch (id) {
    case FP6P2_PROP_RAW:
      p->raw_want = g_value_get_boolean (value);
      break;
    default:
      break;
  }
  g_mutex_unlock (&p->lock);
  return TRUE;
}

static gboolean
fp6p2_get_property (Fp6LibcameraSrc * self, guint id, GValue * value)
{
  Fp6Phase2 *p = self->p2;

  if (id < FP6P2_PROP_BASE || id >= FP6P2_PROP_LAST)
    return FALSE;
  g_mutex_lock (&p->lock);
  switch (id) {
    case FP6P2_PROP_RAW:
      g_value_set_boolean (value, p->raw_want);
      break;
    case FP6P2_PROP_RAW_ACTIVE:
      g_value_set_boolean (value, p->raw_built && p->raw_format);
      break;
    case FP6P2_PROP_RAW_FORMAT:
      g_value_set_string (value, p->raw_format);
      break;
    case FP6P2_PROP_RAW_STATE:
      g_value_set_int (value, p->raw_state);
      break;
    case FP6P2_PROP_RAW_RESULT:
      g_value_set_string (value, p->raw_result);
      break;
    case FP6P2_PROP_RAW_ERROR:
      g_value_set_string (value, p->raw_error);
      break;
    case FP6P2_PROP_SENSOR:
      g_value_set_string (value, fp6_sensor_names[p->sensor]);
      break;
    case FP6P2_PROP_JOB_STATE:
      g_value_set_int (value, p->job_state);
      break;
    case FP6P2_PROP_JOB_KIND:{
      const gchar *k = NULL;
      for (guint i = 0; i < G_N_ELEMENTS (fp6j_kinds); i++)
        if (fp6j_kinds[i].kind == (Fp6JobKind) p->job_kind)
          k = fp6j_kinds[i].name;
      g_value_set_string (value, k);
      break;
    }
    case FP6P2_PROP_JOB_PROGRESS:
      g_value_set_double (value, p->job_state == JOB_CAPTURE
          && p->job_target > 0 ? (gdouble) p->job_frames / p->job_target
          : p->job_progress);
      break;
    case FP6P2_PROP_JOB_FRAMES:
      g_value_set_int (value, p->job_frames);
      break;
    case FP6P2_PROP_JOB_TARGET_FRAMES:
      g_value_set_int (value, p->job_target);
      break;
    case FP6P2_PROP_JOB_RESULT:
      g_value_set_string (value, p->job_result);
      break;
    case FP6P2_PROP_JOB_ERROR:
      g_value_set_string (value, p->job_error);
      break;
    default:
      break;
  }
  g_mutex_unlock (&p->lock);
  return TRUE;
}

static void
fp6p2_class_init (GObjectClass * oc)
{
  const GParamFlags rw = G_PARAM_READWRITE | G_PARAM_STATIC_STRINGS;
  const GParamFlags ro = G_PARAM_READABLE | G_PARAM_STATIC_STRINGS;
  GType t = G_TYPE_FROM_CLASS (oc);

#define P(id, spec) g_object_class_install_property (oc, id, spec)
  P (FP6P2_PROP_RAW, g_param_spec_boolean ("raw", "RAW",
          "Also stream the sensor's raw Bayer frames so raw-capture can "
          "save DNG (next start; costs ~4 raw buffers of memory)", FALSE,
          rw));
  P (FP6P2_PROP_RAW_ACTIVE, g_param_spec_boolean ("raw-active",
          "RAW active", "The running chain has a negotiated raw stream",
          FALSE, ro));
  P (FP6P2_PROP_RAW_FORMAT, g_param_spec_string ("raw-format",
          "RAW format", "Negotiated raw format (e.g. bggr10p)", NULL, ro));
  P (FP6P2_PROP_RAW_STATE, g_param_spec_int ("raw-state", "RAW state",
          "0 idle 1 armed 2 saving 3 done 4 error", 0, 4, 0, ro));
  P (FP6P2_PROP_RAW_RESULT, g_param_spec_string ("raw-result",
          "RAW result", "Path of the last DNG written", NULL, ro));
  P (FP6P2_PROP_RAW_ERROR, g_param_spec_string ("raw-error", "RAW error",
          "Why the last DNG failed", NULL, ro));
  P (FP6P2_PROP_SENSOR, g_param_spec_string ("sensor", "Sensor",
          "Sensor of the built chain: imx896 | ov13b10 | s5kkd1 | unknown",
          NULL, ro));
  P (FP6P2_PROP_JOB_STATE, g_param_spec_int ("job-state", "Job state",
          "0 idle 1 capturing 2 processing 3 done 4 error", 0, 4, 0, ro));
  P (FP6P2_PROP_JOB_KIND, g_param_spec_string ("job-kind", "Job kind",
          "Kind of the current/last job", NULL, ro));
  P (FP6P2_PROP_JOB_PROGRESS, g_param_spec_double ("job-progress",
          "Job progress", "0..1 of the capture (state 1) or the processing "
          "(state 2)", 0, 1, 0, ro));
  P (FP6P2_PROP_JOB_FRAMES, g_param_spec_int ("job-frames", "Job frames",
          "Frames captured", 0, G_MAXINT, 0, ro));
  P (FP6P2_PROP_JOB_TARGET_FRAMES, g_param_spec_int ("job-target-frames",
          "Job target frames", "Frames to capture (0 = open ended)", 0,
          G_MAXINT, 0, ro));
  P (FP6P2_PROP_JOB_RESULT, g_param_spec_string ("job-result", "Job result",
          "Saved file(s), newline separated", NULL, ro));
  P (FP6P2_PROP_JOB_ERROR, g_param_spec_string ("job-error", "Job error",
          "Why the last job failed", NULL, ro));
#undef P

  /* kind, output path, clockwise display rotation, JPEG quality, params */
  g_signal_new_class_handler ("job-start", t,
      G_SIGNAL_RUN_LAST | G_SIGNAL_ACTION, G_CALLBACK (fp6j_start),
      NULL, NULL, NULL, G_TYPE_BOOLEAN, 5, G_TYPE_STRING, G_TYPE_STRING,
      G_TYPE_INT, G_TYPE_INT, G_TYPE_STRING);
  g_signal_new_class_handler ("job-stop", t,
      G_SIGNAL_RUN_LAST | G_SIGNAL_ACTION, G_CALLBACK (fp6j_stop),
      NULL, NULL, NULL, G_TYPE_NONE, 0);
  g_signal_new_class_handler ("job-cancel", t,
      G_SIGNAL_RUN_LAST | G_SIGNAL_ACTION, G_CALLBACK (fp6j_cancel),
      NULL, NULL, NULL, G_TYPE_NONE, 0);

  /* path, clockwise display rotation (as for stack-start) */
  g_signal_new_class_handler ("raw-capture", t,
      G_SIGNAL_RUN_LAST | G_SIGNAL_ACTION, G_CALLBACK (fp6r_capture),
      NULL, NULL, NULL, G_TYPE_BOOLEAN, 2, G_TYPE_STRING, G_TYPE_INT);
}

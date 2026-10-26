/* SPDX-License-Identifier: LGPL-2.1-or-later
 *
 * fp6-imaging-test: offline checks and a file front end for fp6-imaging.c.
 *
 *   fp6-imaging-test selftest [OUTDIR]     synthetic tests (install check)
 *   fp6-imaging-test hdr OUT.ppm IN.ppm... exposure fusion (IN[0] = EV 0)
 */
#include "fp6-imaging.h"

#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* synthetic scene in linear light 0..~4 (bright window, dark corner) */
static gfloat
scene (gint x, gint y, gint c, gint w, gint h)
{
  gfloat fx = (gfloat) x / w, fy = (gfloat) y / h;
  gfloat v = 0.08f + 0.25f * fx;                /* gradient */
  if (fx > 0.55f && fy < 0.45f)
    v = 3.0f + 0.4f * sinf (fx * 60) * sinf (fy * 50);  /* window */
  if (fx < 0.3f && fy > 0.6f)
    v = 0.012f + 0.01f * ((x / 8 + y / 8) & 1);  /* dark checker */
  {
    /* non-periodic block texture (a periodic one aliases the alignment) */
    guint32 hsh = (guint32) (x / 24) * 2654435761u ^ (guint32) (y / 24) * 40503u;
    hsh ^= hsh >> 13;
    hsh *= 0x5bd1e995u;
    if ((hsh >> 7) % 5 == 0)
      v *= 1.6f;
  }
  v *= c == 0 ? 1.1f : c == 2 ? 0.9f : 1.0f;
  return v;
}

static guint8
enc (gfloat lin)
{
  gfloat v = lin <= 0 ? 0 : lin >= 1 ? 1 : powf (lin, 1 / 2.2f);
  return (guint8) (v * 255 + 0.5f);
}

static Fp6Image *
exposure (gint w, gint h, gfloat gain, gint sx, gint sy, gfloat noise,
    guint32 seed)
{
  Fp6Image *img = fp6_image_new (w, h);
  GRand *r = g_rand_new_with_seed (seed);
  for (gint y = 0; y < h; y++)
    for (gint x = 0; x < w; x++)
      for (gint c = 0; c < 3; c++) {
        gfloat v = scene (x + sx, y + sy, c, w, h) * gain;
        if (noise > 0)
          v += (gfloat) g_rand_double_range (r, -noise, noise);
        img->data[(gsize) y * img->stride + 3 * x + c] = enc (v);
      }
  g_rand_free (r);
  return img;
}

/* mean luma and clipped fraction of a region */
static void
region_stats (const Fp6Image * img, gint x0, gint y0, gint x1, gint y1,
    gdouble * mean, gdouble * clipped, gdouble * stddev)
{
  gdouble s = 0, s2 = 0, n = 0, cl = 0;
  for (gint y = y0; y < y1; y++)
    for (gint x = x0; x < x1; x++) {
      const guint8 *p = img->data + (gsize) y * img->stride + 3 * x;
      gdouble l = (p[0] + p[1] + p[2]) / 3.0;
      s += l;
      s2 += l * l;
      n++;
      if (p[0] >= 254 || p[1] >= 254 || p[2] >= 254)
        cl++;
    }
  *mean = s / n;
  *clipped = cl / n;
  *stddev = sqrt (MAX (0, s2 / n - (s / n) * (s / n)));
}

static int
selftest (const gchar * outdir)
{
  const gint w = 640, h = 480;
  Fp6Image *e[3], *out;
  gint dx[3] = { 0 }, dy[3] = { 0 };
  gdouble m, cl, sd;
  gint64 t0;
  int fails = 0;

  /* EV 0, -2 (shifted by 5,-3), +2 (shifted by -7,4) */
  e[0] = exposure (w, h, 1.0f, 0, 0, 0, 1);
  e[1] = exposure (w, h, 0.25f, 5, -3, 0, 2);
  e[2] = exposure (w, h, 4.0f, -7, 4, 0, 3);
  for (gint i = 1; i < 3; i++) {
    fp6_align_mtb_ev (e[0], e[i], i == 1 ? -2 : 2, 64, &dx[i], &dy[i]);
    printf ("mtb %d: %d,%d\n", i, dx[i], dy[i]);
  }
  /* img(x + dx) == ref(x): the -2 frame was rendered at x + 5 */
  if (dx[1] != -5 || dy[1] != 3 || dx[2] != 7 || dy[2] != -4) {
    printf ("FAIL: alignment\n");
    fails++;
  }
  t0 = g_get_monotonic_time ();
  out = fp6_mertens (e, dx, dy, 3, NULL);
  printf ("mertens %dx%d: %.3f s\n", w, h,
      (g_get_monotonic_time () - t0) / 1e6);
  if (!out)
    return 1;
  if (outdir) {
    gchar *p = g_build_filename (outdir, "hdr-ev0.ppm", NULL);
    fp6_image_write_ppm (e[0], p);
    g_free (p);
    p = g_build_filename (outdir, "hdr-fused.ppm", NULL);
    fp6_image_write_ppm (out, p);
    g_free (p);
  }
  /* window: clipped in EV 0, must have detail in the result */
  region_stats (e[0], w * 0.6, h * 0.05, w * 0.95, h * 0.4, &m, &cl, &sd);
  printf ("window EV0: mean %.1f clipped %.2f sd %.1f\n", m, cl, sd);
  region_stats (out, w * 0.6, h * 0.05, w * 0.95, h * 0.4, &m, &cl, &sd);
  printf ("window HDR: mean %.1f clipped %.2f sd %.1f\n", m, cl, sd);
  /* (the scene's brightest texture blocks clip even at EV -2) */
  if (cl > 0.3 || sd < 3) {
    printf ("FAIL: highlights\n");
    fails++;
  }
  /* dark corner: lifted above the EV 0 frame */
  {
    gdouble m0, c0, s0;
    region_stats (e[0], w * 0.02, h * 0.65, w * 0.28, h * 0.95, &m0, &c0, &s0);
    region_stats (out, w * 0.02, h * 0.65, w * 0.28, h * 0.95, &m, &cl, &sd);
    printf ("shadow EV0 %.1f HDR %.1f\n", m0, m);
    if (m < m0 + 5) {
      printf ("FAIL: shadows\n");
      fails++;
    }
  }
  fp6_image_free (out);
  for (gint i = 0; i < 3; i++)
    fp6_image_free (e[i]);

  /* night: 8 noisy frames, shifted, one with a moving bright square */
  {
    Fp6Image *b[8], *res;
    gint ref = -1;
    gdouble m0, c0, s0, m1, c1, s1;
    for (gint i = 0; i < 8; i++) {
      b[i] = exposure (w, h, 0.5f, (i * 37) % 13 - 6, (i * 17) % 9 - 4, 0.06f,
          100 + i);
      if (i == 5)               /* something passing through one frame */
        for (gint y = 200; y < 260; y++)
          for (gint x = 300; x < 360; x++)
            memset (b[i]->data + (gsize) y * b[i]->stride + 3 * x, 250, 3);
    }
    t0 = g_get_monotonic_time ();
    res = fp6_night_merge (b, 8, 1.0, &ref, NULL);
    printf ("night merge %dx%d x8: %.3f s, ref %d\n", w, h,
        (g_get_monotonic_time () - t0) / 1e6, ref);
    if (outdir && res) {
      gchar *p = g_build_filename (outdir, "night-merged.ppm", NULL);
      fp6_image_write_ppm (res, p);
      g_free (p);
      p = g_build_filename (outdir, "night-single.ppm", NULL);
      fp6_image_write_ppm (b[ref], p);
      g_free (p);
    }
    /* error against the noise-free reference frame (interior) */
    {
      Fp6Image *clean = exposure (w, h, 0.5f, (ref * 37) % 13 - 6,
          (ref * 17) % 9 - 4, 0, 1);
      gdouble e0 = 0, e1 = 0, cnt = 0;
      for (gint y = 40; y < h - 40; y++)
        for (gint x = 40; x < w - 40; x++)
          for (gint c = 0; c < 3; c++) {
            gsize o = (gsize) y * clean->stride + 3 * x + c;
            gdouble a = clean->data[o], s1 = b[ref]->data[o], r = res->data[o];
            e0 += (s1 - a) * (s1 - a);
            e1 += (r - a) * (r - a);
            cnt++;
          }
      s0 = sqrt (e0 / cnt);
      s1 = sqrt (e1 / cnt);
      fp6_image_free (clean);
    }
    m0 = m1 = 0;
    printf ("night rms error single %.2f merged %.2f\n", s0, s1);
    if (!res || s1 > 0.6 * s0) {
      printf ("FAIL: night noise\n");
      fails++;
    }
    /* the square of frame 5 must not ghost in */
    if (res && ref != 5) {
      region_stats (res, 310, 210, 350, 250, &m1, &c1, &s1);
      region_stats (b[ref], 310, 210, 350, 250, &m0, &c0, &s0);
      printf ("night ghost region single %.1f merged %.1f\n", m0, m1);
      if (m1 > m0 + 10) {
        printf ("FAIL: night ghost\n");
        fails++;
      }
    }
    fp6_image_free (res);
    for (gint i = 0; i < 8; i++)
      fp6_image_free (b[i]);
  }

  /* rotation round trip */
  {
    Fp6Image *a = exposure (64, 48, 1.0f, 0, 0, 0, 9);
    Fp6Image *b = fp6_image_rotate (a, 90, FALSE);
    Fp6Image *c = fp6_image_rotate (b, 270, FALSE);
    if (b->w != 48 || b->h != 64 || memcmp (a->data, c->data,
            (gsize) a->stride * a->h)) {
      printf ("FAIL: rotate\n");
      fails++;
    }
    /* 90 cw: top-left goes to top-right */
    if (memcmp (b->data + 3 * (b->w - 1), a->data, 3)) {
      printf ("FAIL: rotate direction\n");
      fails++;
    }
    fp6_image_free (a);
    fp6_image_free (b);
    fp6_image_free (c);
  }
  printf (fails ? "selftest FAILED (%d)\n" : "selftest ok\n", fails);
  return fails ? 1 : 0;
}

int
main (int argc, char **argv)
{
  if (argc >= 2 && !strcmp (argv[1], "selftest"))
    return selftest (argc >= 3 ? argv[2] : NULL);
  if (argc >= 5 && !strcmp (argv[1], "hdr")) {
    gint n = argc - 3, dx[16] = { 0 }, dy[16] = { 0 };
    Fp6Image *imgs[16], *out;
    for (gint i = 0; i < n && i < 16; i++) {
      imgs[i] = fp6_image_read_ppm (argv[3 + i]);
      if (!imgs[i]) {
        fprintf (stderr, "cannot read %s\n", argv[3 + i]);
        return 1;
      }
      if (i > 0)
        fp6_align_mtb (imgs[0], imgs[i], 64, &dx[i], &dy[i]);
      printf ("%s: offset %d,%d\n", argv[3 + i], dx[i], dy[i]);
    }
    out = fp6_mertens (imgs, dx, dy, n, NULL);
    return out && fp6_image_write_ppm (out, argv[2]) ? 0 : 1;
  }
  fprintf (stderr, "see the source header for usage\n");
  return 2;
}

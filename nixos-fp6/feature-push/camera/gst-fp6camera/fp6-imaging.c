/* SPDX-License-Identifier: LGPL-2.1-or-later
 *
 * fp6-imaging: CPU image processing for the FP6 camera, see fp6-imaging.h.
 */
#include "fp6-imaging.h"

#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* ------------------------------------------------------------------------ */
/* progress, threads                                                        */

void
fp6_progress (const Fp6Progress * p, gdouble f)
{
  if (p && p->fn)
    p->fn (p->user, p->base + p->span * CLAMP (f, 0.0, 1.0));
}

gboolean
fp6_cancelled (const Fp6Progress * p)
{
  return p && p->cancel && *p->cancel;
}

Fp6Progress
fp6_progress_sub (const Fp6Progress * p, gdouble base, gdouble span)
{
  Fp6Progress s = { NULL, NULL, 0, 1, NULL };
  if (p) {
    s = *p;
    s.base = p->base + p->span * base;
    s.span = p->span * span;
  }
  return s;
}

static gint fp6_threads = 4;

void
fp6_imaging_set_threads (gint n)
{
  fp6_threads = CLAMP (n, 1, 16);
}

typedef struct
{
  void (*fn) (gint, gint, gpointer);
  gpointer user;
  gint y0, y1;
} Band;

static gpointer
band_run (gpointer data)
{
  Band *b = data;
  b->fn (b->y0, b->y1, b->user);
  return NULL;
}

void
fp6_parallel_rows (gint h, void (*fn) (gint, gint, gpointer), gpointer user)
{
  gint n = MIN (fp6_threads, MAX (1, h / 16));
  Band bands[16];
  GThread *t[16];

  if (n <= 1 || h < 64) {
    fn (0, h, user);
    return;
  }
  for (gint i = 0; i < n; i++) {
    bands[i].fn = fn;
    bands[i].user = user;
    bands[i].y0 = (gint) ((gint64) h * i / n);
    bands[i].y1 = (gint) ((gint64) h * (i + 1) / n);
  }
  for (gint i = 1; i < n; i++)
    t[i] = g_thread_new ("fp6-img", band_run, &bands[i]);
  band_run (&bands[0]);
  for (gint i = 1; i < n; i++)
    g_thread_join (t[i]);
}

/* ------------------------------------------------------------------------ */
/* images                                                                   */

Fp6Image *
fp6_image_new (gint w, gint h)
{
  Fp6Image *img;

  if (w <= 0 || h <= 0)
    return NULL;
  img = g_new0 (Fp6Image, 1);
  img->w = w;
  img->h = h;
  img->stride = (w * 3 + 3) & ~3;
  img->data = g_try_malloc0 ((gsize) img->stride * h);
  if (!img->data) {
    g_free (img);
    return NULL;
  }
  return img;
}

void
fp6_image_free (Fp6Image * img)
{
  if (!img)
    return;
  g_free (img->data);
  g_free (img);
}

Fp6Image *
fp6_image_copy (const Fp6Image * img)
{
  Fp6Image *c = fp6_image_new (img->w, img->h);
  if (c)
    memcpy (c->data, img->data, (gsize) img->stride * img->h);
  return c;
}

Fp6Image *
fp6_image_from_packed (const guint8 * src, gint stride, gint w, gint h,
    gint ps, gint ro, gint go, gint bo)
{
  Fp6Image *img = fp6_image_new (w, h);
  if (!img)
    return NULL;
  for (gint y = 0; y < h; y++) {
    const guint8 *s = src + (gsize) y * stride;
    guint8 *d = img->data + (gsize) y * img->stride;
    for (gint x = 0; x < w; x++, s += ps, d += 3) {
      d[0] = s[ro];
      d[1] = s[go];
      d[2] = s[bo];
    }
  }
  return img;
}

Fp6Image *
fp6_image_crop (const Fp6Image * img, gint x, gint y, gint w, gint h)
{
  Fp6Image *c;
  x = CLAMP (x, 0, img->w - 1);
  y = CLAMP (y, 0, img->h - 1);
  w = CLAMP (w, 1, img->w - x);
  h = CLAMP (h, 1, img->h - y);
  c = fp6_image_new (w, h);
  if (!c)
    return NULL;
  for (gint r = 0; r < h; r++)
    memcpy (c->data + (gsize) r * c->stride,
        img->data + (gsize) (y + r) * img->stride + x * 3, (gsize) w * 3);
  return c;
}

Fp6Image *
fp6_image_rotate (const Fp6Image * img, gint rot, gboolean mirror_x)
{
  gint w = img->w, h = img->h;
  Fp6Image *o;

  rot = ((rot % 360) + 360) % 360;
  o = (rot == 90 || rot == 270) ? fp6_image_new (h, w) : fp6_image_new (w, h);
  if (!o)
    return NULL;
  for (gint y = 0; y < h; y++) {
    const guint8 *s = img->data + (gsize) y * img->stride;
    for (gint x = 0; x < w; x++, s += 3) {
      gint mx = mirror_x ? w - 1 - x : x, ox, oy;
      switch (rot) {
        case 90:
          ox = h - 1 - y;
          oy = mx;
          break;
        case 180:
          ox = w - 1 - mx;
          oy = h - 1 - y;
          break;
        case 270:
          ox = y;
          oy = w - 1 - mx;
          break;
        default:
          ox = mx;
          oy = y;
          break;
      }
      memcpy (o->data + (gsize) oy * o->stride + ox * 3, s, 3);
    }
  }
  return o;
}

Fp6Image *
fp6_image_resize (const Fp6Image * img, gint w, gint h)
{
  Fp6Image *o = fp6_image_new (w, h);
  gdouble sx = (gdouble) img->w / w, sy = (gdouble) img->h / h;

  if (!o)
    return NULL;
  for (gint y = 0; y < h; y++) {
    gdouble fy = CLAMP ((y + 0.5) * sy - 0.5, 0, img->h - 1);
    gint y0 = (gint) fy, y1 = MIN (y0 + 1, img->h - 1);
    gfloat ay = fy - y0;
    guint8 *d = o->data + (gsize) y * o->stride;
    for (gint x = 0; x < w; x++) {
      gdouble fx = CLAMP ((x + 0.5) * sx - 0.5, 0, img->w - 1);
      gint x0 = (gint) fx, x1 = MIN (x0 + 1, img->w - 1);
      gfloat ax = fx - x0;
      const guint8 *a = img->data + (gsize) y0 * img->stride;
      const guint8 *b = img->data + (gsize) y1 * img->stride;
      for (gint c = 0; c < 3; c++) {
        gfloat t = a[x0 * 3 + c] * (1 - ax) + a[x1 * 3 + c] * ax;
        gfloat u = b[x0 * 3 + c] * (1 - ax) + b[x1 * 3 + c] * ax;
        d[x * 3 + c] = (guint8) (t * (1 - ay) + u * ay + 0.5f);
      }
    }
  }
  return o;
}

gboolean
fp6_image_write_ppm (const Fp6Image * img, const gchar * path)
{
  FILE *f = fopen (path, "wb");
  gboolean ok;
  if (!f)
    return FALSE;
  ok = fprintf (f, "P6\n%d %d\n255\n", img->w, img->h) > 0;
  for (gint y = 0; ok && y < img->h; y++)
    ok = fwrite (img->data + (gsize) y * img->stride, 3, img->w, f) ==
        (gsize) img->w;
  return fclose (f) == 0 && ok;
}

static gint
ppm_int (FILE * f)
{
  gint c, v = 0;
  do {
    c = fgetc (f);
    if (c == '#')
      while (c != '\n' && c != EOF)
        c = fgetc (f);
  } while (c == ' ' || c == '\n' || c == '\r' || c == '\t');
  while (c >= '0' && c <= '9') {
    v = v * 10 + (c - '0');
    c = fgetc (f);
  }
  return v;
}

Fp6Image *
fp6_image_read_ppm (const gchar * path)
{
  FILE *f = fopen (path, "rb");
  Fp6Image *img = NULL;
  gint w, h, max;

  if (!f)
    return NULL;
  if (fgetc (f) == 'P' && fgetc (f) == '6') {
    w = ppm_int (f);
    h = ppm_int (f);
    max = ppm_int (f);
    if (w > 0 && h > 0 && max == 255 && (img = fp6_image_new (w, h))) {
      for (gint y = 0; y < h; y++)
        if (fread (img->data + (gsize) y * img->stride, 3, w, f) != (gsize) w) {
          fp6_image_free (img);
          img = NULL;
          break;
        }
    }
  }
  fclose (f);
  return img;
}

Fp6Gray *
fp6_gray_new (gint w, gint h)
{
  Fp6Gray *g = g_new0 (Fp6Gray, 1);
  g->w = MAX (w, 1);
  g->h = MAX (h, 1);
  g->d = g_malloc0 ((gsize) g->w * g->h);
  return g;
}

void
fp6_gray_free (Fp6Gray * g)
{
  if (!g)
    return;
  g_free (g->d);
  g_free (g);
}

Fp6Gray *
fp6_gray_from_image (const Fp6Image * img, gint shift)
{
  gint f = 1 << shift, w = img->w >> shift, h = img->h >> shift;
  Fp6Gray *g = fp6_gray_new (w, h);

  for (gint y = 0; y < g->h; y++) {
    for (gint x = 0; x < g->w; x++) {
      guint sum = 0;
      for (gint j = 0; j < f; j++) {
        const guint8 *p = img->data + (gsize) (y * f + j) * img->stride +
            (gsize) x * f * 3;
        for (gint i = 0; i < f; i++, p += 3)
          sum += p[0] * 77 + p[1] * 150 + p[2] * 29;
      }
      g->d[(gsize) y * g->w + x] = (guint8) (sum >> (8 + 2 * shift));
    }
  }
  return g;
}

Fp6Gray *
fp6_gray_half (const Fp6Gray * s)
{
  Fp6Gray *g = fp6_gray_new (s->w / 2, s->h / 2);
  for (gint y = 0; y < g->h; y++) {
    const guint8 *a = s->d + (gsize) (2 * y) * s->w;
    const guint8 *b = a + s->w;
    guint8 *d = g->d + (gsize) y * g->w;
    for (gint x = 0; x < g->w; x++)
      d[x] = (a[2 * x] + a[2 * x + 1] + b[2 * x] + b[2 * x + 1] + 2) >> 2;
  }
  return g;
}

/* ------------------------------------------------------------------------ */
/* MTB alignment                                                            */

static gint
gray_median (const Fp6Gray * g)
{
  guint hist[256] = { 0 };
  gsize n = (gsize) g->w * g->h, acc = 0;
  for (gsize i = 0; i < n; i++)
    hist[g->d[i]]++;
  for (gint v = 0; v < 256; v++) {
    acc += hist[v];
    if (acc * 2 >= n)
      return v;
  }
  return 128;
}

/* threshold bitmap (bit 0) and exclusion bitmap (bit 1, 1 = usable) */
static guint8 *
mtb_bitmaps (const Fp6Gray * g)
{
  gint med = gray_median (g);
  gsize n = (gsize) g->w * g->h;
  guint8 *b = g_malloc (n);
  /* keep the threshold away from the extremes: a mostly clipped frame
   * would otherwise have no usable bits */
  med = CLAMP (med, 8, 247);
  for (gsize i = 0; i < n; i++) {
    gint v = g->d[i];
    b[i] = (v > med ? 1 : 0) | (abs (v - med) > 4 ? 2 : 0);
  }
  return b;
}

static guint64
mtb_error (const guint8 * a, const guint8 * b, gint w, gint h, gint ox,
    gint oy, gint margin)
{
  guint64 err = 0;
  for (gint y = margin; y < h - margin; y++) {
    const guint8 *pa = a + (gsize) y * w;
    const guint8 *pb = b + (gsize) (y + oy) * w + ox;
    for (gint x = margin; x < w - margin; x++) {
      guint8 u = pa[x], v = pb[x];
      err += ((u ^ v) & 1) & (u >> 1) & (v >> 1);
    }
  }
  return err;
}

static gint
mtb_best (const Fp6Gray * r, const Fp6Gray * i, gint cx, gint cy, gint * bx,
    gint * by)
{
  guint8 *a = mtb_bitmaps (r), *b = mtb_bitmaps (i);
  gint margin = MAX (ABS (cx), ABS (cy)) + 2;
  guint64 best = G_MAXUINT64;

  *bx = *by = 0;
  if (2 * margin + 8 < MIN (r->w, r->h)) {
    for (gint sy = -1; sy <= 1; sy++)
      for (gint sx = -1; sx <= 1; sx++) {
        guint64 e = mtb_error (a, b, r->w, r->h, cx + sx, cy + sy, margin);
        /* prefer no extra shift on ties */
        if (e < best || (e == best && sx == 0 && sy == 0)) {
          best = e;
          *bx = sx;
          *by = sy;
        }
      }
  }
  g_free (a);
  g_free (b);
  return 0;
}

/* brighten a gamma-encoded luma image by 2^ev with clipping, so that a
 * darker frame clips where the brighter one does */
static void
gray_expose (Fp6Gray * g, gdouble ev)
{
  guint8 lut[256];
  gdouble f = pow (2.0, ev);
  gsize n = (gsize) g->w * g->h;
  for (gint v = 0; v < 256; v++) {
    gdouble l = pow (v / 255.0, 2.2) * f;
    lut[v] = l >= 1 ? 255 : (guint8) (255 * pow (l, 1 / 2.2) + 0.5);
  }
  for (gsize i = 0; i < n; i++)
    g->d[i] = lut[g->d[i]];
}

void
fp6_align_mtb_ev (const Fp6Image * ref, const Fp6Image * img, gdouble ev,
    gint max_shift, gint * dx, gint * dy)
{
  gint levels = 0, cx = 0, cy = 0, bx, by;
  Fp6Gray *fr = fp6_gray_from_image (ref, 0), *fi = fp6_gray_from_image (img, 0);
  Fp6Gray *pr[12], *pi[12];

  if (ev > 0.05)
    gray_expose (fr, ev);
  else if (ev < -0.05)
    gray_expose (fi, -ev);
  while ((2 << levels) <= max_shift && levels < 10)
    levels++;
  /* level 0 = half resolution, refined at full resolution at the end */
  pr[0] = fp6_gray_half (fr);
  pi[0] = fp6_gray_half (fi);
  for (gint l = 1; l <= levels; l++) {
    pr[l] = fp6_gray_half (pr[l - 1]);
    pi[l] = fp6_gray_half (pi[l - 1]);
  }
  for (gint l = levels; l >= 0; l--) {
    cx *= 2;
    cy *= 2;
    mtb_best (pr[l], pi[l], cx, cy, &bx, &by);
    cx += bx;
    cy += by;
  }
  for (gint l = 0; l <= levels; l++) {
    fp6_gray_free (pr[l]);
    fp6_gray_free (pi[l]);
  }
  cx *= 2;
  cy *= 2;
  mtb_best (fr, fi, cx, cy, &bx, &by);
  *dx = cx + bx;
  *dy = cy + by;
  fp6_gray_free (fr);
  fp6_gray_free (fi);
}

void
fp6_align_mtb (const Fp6Image * ref, const Fp6Image * img, gint max_shift,
    gint * dx, gint * dy)
{
  fp6_align_mtb_ev (ref, img, 0, max_shift, dx, dy);
}

/* ------------------------------------------------------------------------ */
/* float images and Gaussian/Laplacian pyramids                             */

typedef struct
{
  gint w, h, c;
  gfloat *d;
} Fl;

static Fl *
fl_new (gint w, gint h, gint c)
{
  Fl *f = g_new0 (Fl, 1);
  f->w = w;
  f->h = h;
  f->c = c;
  f->d = g_try_new0 (gfloat, (gsize) w * h * c);
  if (!f->d) {
    g_free (f);
    return NULL;
  }
  return f;
}

static void
fl_free (Fl * f)
{
  if (!f)
    return;
  g_free (f->d);
  g_free (f);
}

typedef struct
{
  const Fl *s;
  Fl *d;
  gfloat *tmp;                  /* dw x sh x c */
} PyrJob;

/* horizontal pass: decimate columns with [1 4 6 4 1]/16 */
static void
pyr_down_h (gint y0, gint y1, gpointer user)
{
  PyrJob *j = user;
  const Fl *s = j->s;
  gint c = s->c, dw = j->d->w, sw = s->w;
  for (gint y = y0; y < y1; y++) {
    const gfloat *r = s->d + (gsize) y * sw * c;
    gfloat *o = j->tmp + (gsize) y * dw * c;
    for (gint x = 0; x < dw; x++) {
      gint x0 = 2 * x, xm2 = MAX (x0 - 2, 0), xm1 = MAX (x0 - 1, 0),
          xp1 = MIN (x0 + 1, sw - 1), xp2 = MIN (x0 + 2, sw - 1);
      for (gint k = 0; k < c; k++)
        o[x * c + k] = (r[xm2 * c + k] + r[xp2 * c + k]
            + 4.0f * (r[xm1 * c + k] + r[xp1 * c + k])
            + 6.0f * r[x0 * c + k]) * (1.0f / 16.0f);
    }
  }
}

static void
pyr_down_v (gint y0, gint y1, gpointer user)
{
  PyrJob *j = user;
  gint c = j->s->c, dw = j->d->w, sh = j->s->h, n = dw * c;
  for (gint y = y0; y < y1; y++) {
    gint r0 = 2 * y;
    const gfloat *a = j->tmp + (gsize) MAX (r0 - 2, 0) * n;
    const gfloat *b = j->tmp + (gsize) MAX (r0 - 1, 0) * n;
    const gfloat *m = j->tmp + (gsize) r0 * n;
    const gfloat *e = j->tmp + (gsize) MIN (r0 + 1, sh - 1) * n;
    const gfloat *f = j->tmp + (gsize) MIN (r0 + 2, sh - 1) * n;
    gfloat *o = j->d->d + (gsize) y * n;
    for (gint i = 0; i < n; i++)
      o[i] = (a[i] + f[i] + 4.0f * (b[i] + e[i]) + 6.0f * m[i]) *
          (1.0f / 16.0f);
  }
}

static Fl *
pyr_down (const Fl * s)
{
  PyrJob j;
  Fl *d = fl_new ((s->w + 1) / 2, (s->h + 1) / 2, s->c);
  if (!d)
    return NULL;
  j.s = s;
  j.d = d;
  j.tmp = g_try_new (gfloat, (gsize) d->w * s->h * s->c);
  if (!j.tmp) {
    fl_free (d);
    return NULL;
  }
  fp6_parallel_rows (s->h, pyr_down_h, &j);
  fp6_parallel_rows (d->h, pyr_down_v, &j);
  g_free (j.tmp);
  return d;
}

typedef struct
{
  const Fl *s;
  gint dw, dh;
  gfloat *tmp;                  /* dw x sh x c */
  gfloat *out;                  /* dw x dh x c */
  gfloat sign;                  /* out = out + sign * up(s) (accumulate) */
  gboolean accumulate;
} UpJob;

/* expand: even x = (s[i-1] + 6 s[i] + s[i+1]) / 8, odd = (s[i] + s[i+1]) / 2 */
static void
pyr_up_h (gint y0, gint y1, gpointer user)
{
  UpJob *j = user;
  const Fl *s = j->s;
  gint c = s->c, sw = s->w;
  for (gint y = y0; y < y1; y++) {
    const gfloat *r = s->d + (gsize) y * sw * c;
    gfloat *o = j->tmp + (gsize) y * j->dw * c;
    for (gint x = 0; x < j->dw; x++) {
      gint i = x >> 1;
      gint im = MAX (i - 1, 0), i0 = MIN (i, sw - 1), ip = MIN (i + 1, sw - 1);
      if (x & 1)
        for (gint k = 0; k < c; k++)
          o[x * c + k] = 0.5f * (r[i0 * c + k] + r[ip * c + k]);
      else
        for (gint k = 0; k < c; k++)
          o[x * c + k] = 0.125f * (r[im * c + k] + r[ip * c + k])
              + 0.75f * r[i0 * c + k];
    }
  }
}

static void
pyr_up_v (gint y0, gint y1, gpointer user)
{
  UpJob *j = user;
  gint sh = j->s->h, n = j->dw * j->s->c;
  for (gint y = y0; y < y1; y++) {
    gint i = y >> 1;
    const gfloat *a = j->tmp + (gsize) MAX (i - 1, 0) * n;
    const gfloat *m = j->tmp + (gsize) MIN (i, sh - 1) * n;
    const gfloat *b = j->tmp + (gsize) MIN (i + 1, sh - 1) * n;
    gfloat *o = j->out + (gsize) y * n;
    if (y & 1) {
      for (gint k = 0; k < n; k++)
        o[k] += j->sign * 0.5f * (m[k] + b[k]);
    } else {
      for (gint k = 0; k < n; k++)
        o[k] += j->sign * (0.125f * (a[k] + b[k]) + 0.75f * m[k]);
    }
  }
}

/* out (dw x dh) += sign * expand (s) */
static gboolean
pyr_up_add (const Fl * s, Fl * out, gfloat sign)
{
  UpJob j = { s, out->w, out->h, NULL, out->d, sign, TRUE };
  j.tmp = g_try_new (gfloat, (gsize) out->w * s->h * s->c);
  if (!j.tmp)
    return FALSE;
  fp6_parallel_rows (s->h, pyr_up_h, &j);
  fp6_parallel_rows (out->h, pyr_up_v, &j);
  g_free (j.tmp);
  return TRUE;
}

/* ------------------------------------------------------------------------ */
/* exposure fusion                                                          */

typedef struct
{
  const Fp6Image *img;
  gint dx, dy;
  Fl *rgb;                      /* out: 3 ch 0..1 */
  Fl *w;                        /* out: weight */
} WeightJob;

static inline const guint8 *
px_at (const Fp6Image * img, gint x, gint y)
{
  x = CLAMP (x, 0, img->w - 1);
  y = CLAMP (y, 0, img->h - 1);
  return img->data + (gsize) y * img->stride + x * 3;
}

static void
mertens_weights (gint y0, gint y1, gpointer user)
{
  WeightJob *j = user;
  const Fp6Image *img = j->img;
  gint w = img->w, h = img->h;
  const gfloat inv = 1.0f / 255.0f;
  static const gfloat sig2 = 2.0f * 0.2f * 0.2f;

  for (gint y = y0; y < y1; y++) {
    gfloat *o = j->rgb->d + (gsize) y * w * 3;
    gfloat *wo = j->w->d + (gsize) y * w;
    for (gint x = 0; x < w; x++) {
      const guint8 *p = px_at (img, x + j->dx, y + j->dy);
      const guint8 *pl = px_at (img, x + j->dx - 1, y + j->dy);
      const guint8 *pr = px_at (img, x + j->dx + 1, y + j->dy);
      const guint8 *pu = px_at (img, x + j->dx, y + j->dy - 1);
      const guint8 *pd = px_at (img, x + j->dx, y + j->dy + 1);
      gfloat r = p[0] * inv, g = p[1] * inv, b = p[2] * inv;
      gfloat m = (r + g + b) * (1.0f / 3.0f);
      gfloat lap = ((pl[0] + pl[1] + pl[2]) + (pr[0] + pr[1] + pr[2])
          + (pu[0] + pu[1] + pu[2]) + (pd[0] + pd[1] + pd[2])) * (inv / 3.0f)
          - 4.0f * m;
      gfloat sat = sqrtf (((r - m) * (r - m) + (g - m) * (g - m)
              + (b - m) * (b - m)) * (1.0f / 3.0f));
      gfloat e = expf (-((r - 0.5f) * (r - 0.5f) + (g - 0.5f) * (g - 0.5f)
              + (b - 0.5f) * (b - 0.5f)) / sig2);
      o[3 * x] = r;
      o[3 * x + 1] = g;
      o[3 * x + 2] = b;
      /* (y0 > 0 rows of other bands read only the source image) */
      wo[x] = fabsf (lap) * sat * e + 1e-6f;
      (void) h;
    }
  }
}

typedef struct
{
  Fl **w;
  gint n;
  gsize len;
  gint width;
} NormJob;

static void
mertens_normalise (gint y0, gint y1, gpointer user)
{
  NormJob *j = user;
  for (gsize i = (gsize) y0 * j->width; i < (gsize) y1 * j->width; i++) {
    gfloat s = 0;
    for (gint k = 0; k < j->n; k++)
      s += j->w[k]->d[i];
    s = 1.0f / s;
    for (gint k = 0; k < j->n; k++)
      j->w[k]->d[i] *= s;
  }
}

typedef struct
{
  Fl *acc;                      /* 3 ch */
  const Fl *lap;                /* 3 ch */
  const Fl *w;                  /* 1 ch */
} BlendJob;

static void
mertens_blend (gint y0, gint y1, gpointer user)
{
  BlendJob *j = user;
  gint w = j->acc->w;
  for (gint y = y0; y < y1; y++) {
    gfloat *a = j->acc->d + (gsize) y * w * 3;
    const gfloat *l = j->lap->d + (gsize) y * w * 3;
    const gfloat *wt = j->w->d + (gsize) y * w;
    for (gint x = 0; x < w; x++) {
      a[3 * x] += wt[x] * l[3 * x];
      a[3 * x + 1] += wt[x] * l[3 * x + 1];
      a[3 * x + 2] += wt[x] * l[3 * x + 2];
    }
  }
}

#define MAX_LEVELS 12

Fp6Image *
fp6_mertens (Fp6Image * const *imgs, const gint * dx, const gint * dy,
    gint n, const Fp6Progress * prog)
{
  gint w = imgs[0]->w, h = imgs[0]->h, levels = 1;
  Fl *wts[16] = { NULL }, *rgb[16] = { NULL };
  Fl *acc[MAX_LEVELS] = { NULL };
  Fp6Image *out = NULL;
  gboolean ok = TRUE;

  if (n < 1 || n > 16)
    return NULL;
  while ((MIN (w, h) >> (levels + 3)) >= 1 && levels < MAX_LEVELS)
    levels++;

  /* 1. weights (and the float images) */
  for (gint k = 0; k < n && ok; k++) {
    WeightJob j = { imgs[k], dx ? dx[k] : 0, dy ? dy[k] : 0,
      fl_new (w, h, 3), fl_new (w, h, 1)
    };
    rgb[k] = j.rgb;
    wts[k] = j.w;
    if (!j.rgb || !j.w || imgs[k]->w != w || imgs[k]->h != h) {
      ok = FALSE;
      break;
    }
    fp6_parallel_rows (h, mertens_weights, &j);
    fp6_progress (prog, 0.15 * (k + 1) / n);
  }
  if (ok) {
    NormJob j = { wts, n, (gsize) w * h, w };
    fp6_parallel_rows (h, mertens_normalise, &j);
  }

  /* 2. result pyramid */
  for (gint l = 0; ok && l < levels; l++) {
    acc[l] = fl_new (MAX (1, (w + (1 << l) - 1) >> l),
        MAX (1, (h + (1 << l) - 1) >> l), 3);
    ok = acc[l] != NULL;
  }

  /* 3. per image: Laplacian pyramid of the image, Gaussian of the weight */
  for (gint k = 0; ok && k < n && !fp6_cancelled (prog); k++) {
    Fl *g[MAX_LEVELS] = { NULL }, *wp[MAX_LEVELS] = { NULL };
    g[0] = rgb[k];
    wp[0] = wts[k];
    rgb[k] = wts[k] = NULL;
    for (gint l = 1; ok && l < levels; l++) {
      g[l] = pyr_down (g[l - 1]);
      wp[l] = pyr_down (wp[l - 1]);
      ok = g[l] && wp[l];
    }
    /* Laplacian in place, finest first (needs the coarser Gaussian) */
    for (gint l = 0; ok && l < levels - 1; l++)
      ok = pyr_up_add (g[l + 1], g[l], -1.0f);
    for (gint l = 0; ok && l < levels; l++) {
      BlendJob j = { acc[l], g[l], wp[l] };
      fp6_parallel_rows (acc[l]->h, mertens_blend, &j);
    }
    for (gint l = 0; l < levels; l++) {
      fl_free (g[l]);
      fl_free (wp[l]);
    }
    fp6_progress (prog, 0.15 + 0.75 * (k + 1) / n);
  }

  /* 4. collapse */
  for (gint l = levels - 2; ok && l >= 0; l--)
    ok = pyr_up_add (acc[l + 1], acc[l], 1.0f);
  if (ok && !fp6_cancelled (prog) && (out = fp6_image_new (w, h))) {
    for (gint y = 0; y < h; y++) {
      const gfloat *a = acc[0]->d + (gsize) y * w * 3;
      guint8 *o = out->data + (gsize) y * out->stride;
      for (gint i = 0; i < w * 3; i++) {
        gfloat v = a[i] * 255.0f + 0.5f;
        o[i] = v <= 0 ? 0 : v >= 255 ? 255 : (guint8) v;
      }
    }
  }
  fp6_progress (prog, 1.0);
  for (gint k = 0; k < n; k++) {
    fl_free (rgb[k]);
    fl_free (wts[k]);
  }
  for (gint l = 0; l < levels; l++)
    fl_free (acc[l]);
  return out;
}

/* ------------------------------------------------------------------------ */
/* translation alignment (same exposure): coarse-to-fine SAD                */

static guint64
sad_at (const Fp6Gray * a, const Fp6Gray * b, gint ox, gint oy, gint margin,
    gint step)
{
  guint64 s = 0;
  for (gint y = margin; y < a->h - margin; y += step) {
    const guint8 *pa = a->d + (gsize) y * a->w;
    const guint8 *pb = b->d + (gsize) (y + oy) * b->w + ox;
    for (gint x = margin; x < a->w - margin; x += step)
      s += abs ((gint) pa[x] - (gint) pb[x]);
  }
  return s;
}

void
fp6_align_translation (const Fp6Gray * ref, const Fp6Gray * img,
    gint max_shift, gint * dx, gint * dy)
{
  Fp6Gray *pr[12], *pi[12];
  gint levels = 0, cx = 0, cy = 0;

  while ((4 << levels) <= max_shift && levels < 8
      && (MIN (ref->w, ref->h) >> (levels + 1)) >= 32)
    levels++;
  pr[0] = (Fp6Gray *) ref;
  pi[0] = (Fp6Gray *) img;
  for (gint l = 1; l <= levels; l++) {
    pr[l] = fp6_gray_half (pr[l - 1]);
    pi[l] = fp6_gray_half (pi[l - 1]);
  }
  for (gint l = levels; l >= 0; l--) {
    gint r = l == levels ? 4 : 1, bx = 0, by = 0;
    gint margin, step = l == 0 ? 2 : 1;
    guint64 best = G_MAXUINT64;
    cx *= 2;
    cy *= 2;
    if (l == levels)
      cx = cy = 0;
    margin = MAX (ABS (cx), ABS (cy)) + r + 1;
    if (2 * margin + 8 >= MIN (pr[l]->w, pr[l]->h))
      continue;
    for (gint sy = -r; sy <= r; sy++)
      for (gint sx = -r; sx <= r; sx++) {
        guint64 e = sad_at (pr[l], pi[l], cx + sx, cy + sy, margin, step);
        if (e < best || (e == best && sx == 0 && sy == 0)) {
          best = e;
          bx = sx;
          by = sy;
        }
      }
    cx += bx;
    cy += by;
  }
  for (gint l = 1; l <= levels; l++) {
    fp6_gray_free (pr[l]);
    fp6_gray_free (pi[l]);
  }
  *dx = cx;
  *dy = cy;
}

/* ------------------------------------------------------------------------ */
/* night: burst merge                                                       */

#define NM_TILE 64

static gfloat nm_lin[256];
static guint8 nm_enc[4096];

static void
nm_tables (void)
{
  static gsize once = 0;
  if (g_once_init_enter (&once)) {
    /* the soft ISP's output curve is sRGB (camera phase 0, 0018) */
    for (gint i = 0; i < 256; i++) {
      gdouble v = i / 255.0;
      nm_lin[i] = v <= 0.04045 ? v / 12.92 : pow ((v + 0.055) / 1.055, 2.4);
    }
    for (gint i = 0; i < 4096; i++) {
      gdouble l = i / 4095.0;
      gdouble v = l <= 0.0031308 ? 12.92 * l : 1.055 * pow (l, 1 / 2.4) - 0.055;
      nm_enc[i] = (guint8) CLAMP (lround (v * 255), 0, 255);
    }
    g_once_init_leave (&once, 1);
  }
}

typedef struct
{
  const Fp6Image *ref, *img;
  const Fp6Gray *gref, *gimg;
  gint tw, th;                  /* tiles */
  gint *tdx, *tdy;              /* per tile shift */
  gint gdx, gdy;                /* global shift */
  gint16 *diff;                 /* per pixel luma difference, w * h */
  gfloat *acc, *wsum;           /* w * h * 3, w * h */
  gfloat sigma;                 /* difference noise (luma codes) */
  gfloat boost;
  gfloat black;                 /* linear offset removed from the result */
} NmJob;

/* per tile: refine the global shift by +-2 px (full-resolution SAD) */
static void
nm_tiles (gint t0, gint t1, gpointer user)
{
  NmJob *j = user;
  const Fp6Gray *a = j->gref, *b = j->gimg;
  for (gint t = t0; t < t1; t++) {
    gint tx = (t % j->tw) * NM_TILE, ty = (t / j->tw) * NM_TILE;
    gint x1 = MIN (tx + NM_TILE, a->w), y1 = MIN (ty + NM_TILE, a->h);
    guint64 best = G_MAXUINT64;
    gint bx = j->gdx, by = j->gdy;
    for (gint sy = -2; sy <= 2; sy++)
      for (gint sx = -2; sx <= 2; sx++) {
        gint ox = j->gdx + sx, oy = j->gdy + sy;
        guint64 s = 0;
        if (tx + ox < 0 || ty + oy < 0 || x1 + ox > b->w || y1 + oy > b->h)
          continue;
        for (gint y = ty; y < y1; y += 2) {
          const guint8 *pa = a->d + (gsize) y * a->w;
          const guint8 *pb = b->d + (gsize) (y + oy) * b->w + ox;
          for (gint x = tx; x < x1; x += 2)
            s += abs ((gint) pa[x] - (gint) pb[x]);
        }
        if (s < best || (s == best && sx == 0 && sy == 0)) {
          best = s;
          bx = ox;
          by = oy;
        }
      }
    j->tdx[t] = bx;
    j->tdy[t] = by;
  }
}

static inline void
nm_shift (const NmJob * j, gint x, gint y, gint * sx, gint * sy)
{
  gint t = (y / NM_TILE) * j->tw + x / NM_TILE;
  *sx = CLAMP (x + j->tdx[t], 0, j->img->w - 1);
  *sy = CLAMP (y + j->tdy[t], 0, j->img->h - 1);
}

static void
nm_diff (gint y0, gint y1, gpointer user)
{
  NmJob *j = user;
  gint w = j->ref->w;
  for (gint y = y0; y < y1; y++)
    for (gint x = 0; x < w; x++) {
      gint sx, sy;
      nm_shift (j, x, y, &sx, &sy);
      j->diff[(gsize) y * w + x] = (gint16) j->gimg->d[(gsize) sy * w + sx]
          - (gint16) j->gref->d[(gsize) y * w + x];
    }
}

/* accumulate one frame with a robustness weight from the 5x5 mean of the
 * luma difference (local motion / misalignment -> weight 0) */
static void
nm_accumulate (gint y0, gint y1, gpointer user)
{
  NmJob *j = user;
  gint w = j->ref->w, h = j->ref->h;
  /* 5x5 mean of noise has sigma / 5; accept up to ~2.5 of those */
  gfloat lo = 1.5f * j->sigma / 5.0f + 1.0f, hi = 4.0f * j->sigma / 5.0f + 2.0f;
  for (gint y = y0; y < y1; y++) {
    gint ya = MAX (y - 2, 0), yb = MIN (y + 2, h - 1);
    for (gint x = 0; x < w; x++) {
      gint xa = MAX (x - 2, 0), xb = MIN (x + 2, w - 1), sx, sy, n = 0;
      gint sum = 0;
      gfloat d, wt;
      for (gint yy = ya; yy <= yb; yy++)
        for (gint xx = xa; xx <= xb; xx++, n++)
          sum += j->diff[(gsize) yy * w + xx];
      d = fabsf ((gfloat) sum / n);
      wt = d <= lo ? 1.0f : d >= hi ? 0.0f : (hi - d) / (hi - lo);
      if (wt <= 0)
        continue;
      nm_shift (j, x, y, &sx, &sy);
      {
        const guint8 *p = j->img->data + (gsize) sy * j->img->stride + 3 * sx;
        gfloat *a = j->acc + ((gsize) y * w + x) * 3;
        a[0] += wt * nm_lin[p[0]];
        a[1] += wt * nm_lin[p[1]];
        a[2] += wt * nm_lin[p[2]];
        j->wsum[(gsize) y * w + x] += wt;
      }
    }
  }
}

static void
nm_reference (gint y0, gint y1, gpointer user)
{
  NmJob *j = user;
  gint w = j->ref->w;
  for (gint y = y0; y < y1; y++) {
    const guint8 *p = j->ref->data + (gsize) y * j->ref->stride;
    gfloat *a = j->acc + (gsize) y * w * 3;
    for (gint x = 0; x < w; x++) {
      a[3 * x] = nm_lin[p[3 * x]];
      a[3 * x + 1] = nm_lin[p[3 * x + 1]];
      a[3 * x + 2] = nm_lin[p[3 * x + 2]];
      j->wsum[(gsize) y * w + x] = 1.0f;
    }
  }
}

typedef struct
{
  const NmJob *j;
  Fp6Image *out;
} NmOut;

static void
nm_output (gint y0, gint y1, gpointer user)
{
  NmOut *o = user;
  const NmJob *j = o->j;
  gint w = o->out->w;
  for (gint y = y0; y < y1; y++) {
    guint8 *d = o->out->data + (gsize) y * o->out->stride;
    const gfloat *a = j->acc + (gsize) y * w * 3;
    const gfloat *ws = j->wsum + (gsize) y * w;
    for (gint x = 0; x < w; x++) {
      gfloat k = j->boost / ws[x], sc = 1.0f / (1.0f - j->black);
      for (gint c = 0; c < 3; c++) {
        gint i = (gint) ((a[3 * x + c] * k - j->black) * sc * 4095.0f + 0.5f);
        d[3 * x + c] = nm_enc[CLAMP (i, 0, 4095)];
      }
    }
  }
}

static gdouble
gray_sharpness (const Fp6Gray * g)
{
  guint64 s = 0;
  for (gint y = 1; y < g->h - 1; y += 2)
    for (gint x = 1; x < g->w - 1; x += 2) {
      const guint8 *p = g->d + (gsize) y * g->w + x;
      gint gx = p[1] - p[-1], gy = p[g->w] - p[-g->w];
      s += gx * gx + gy * gy;
    }
  return (gdouble) s;
}

static int
cmp_i16 (const void *a, const void *b)
{
  return *(const gint16 *) a - *(const gint16 *) b;
}

Fp6Image *
fp6_night_merge (Fp6Image * const *imgs, gint n, gdouble boost,
    gint * ref_out, const Fp6Progress * prog)
{
  gint w = imgs[0]->w, h = imgs[0]->h, ref = 0;
  Fp6Gray **g = g_new0 (Fp6Gray *, n);
  NmJob j = { 0 };
  NmOut o;
  gdouble best = -1;
  gboolean ok = TRUE;

  nm_tables ();
  for (gint i = 0; i < n; i++) {
    g[i] = fp6_gray_from_image (imgs[i], 0);
    /* reference: the sharpest of the first frames (least shake) */
    if (i < MIN (n, 4)) {
      Fp6Gray *half = fp6_gray_half (g[i]);
      gdouble s = gray_sharpness (half);
      fp6_gray_free (half);
      if (s > best) {
        best = s;
        ref = i;
      }
    }
  }
  if (ref_out)
    *ref_out = ref;
  j.ref = imgs[ref];
  j.gref = g[ref];
  j.tw = (w + NM_TILE - 1) / NM_TILE;
  j.th = (h + NM_TILE - 1) / NM_TILE;
  j.tdx = g_new0 (gint, j.tw * j.th);
  j.tdy = g_new0 (gint, j.tw * j.th);
  j.diff = g_try_new (gint16, (gsize) w * h);
  j.acc = g_try_new (gfloat, (gsize) w * h * 3);
  j.wsum = g_try_new (gfloat, (gsize) w * h);
  j.boost = boost > 0 ? boost : 1.0;
  ok = j.diff && j.acc && j.wsum;
  if (ok)
    fp6_parallel_rows (h, nm_reference, &j);

  for (gint i = 0; ok && i < n && !fp6_cancelled (prog); i++) {
    if (i == ref || imgs[i]->w != w || imgs[i]->h != h)
      continue;
    j.img = imgs[i];
    j.gimg = g[i];
    {
      Fp6Gray *a = fp6_gray_half (g[ref]), *b = fp6_gray_half (g[i]);
      fp6_align_translation (a, b, 48, &j.gdx, &j.gdy);
      j.gdx *= 2;
      j.gdy *= 2;
      fp6_gray_free (a);
      fp6_gray_free (b);
    }
    fp6_parallel_rows (j.tw * j.th, nm_tiles, &j);
    fp6_parallel_rows (h, nm_diff, &j);
    /* noise of the difference: robust from the median absolute value */
    {
      gsize m = 0, cnt = ((gsize) w * h) / 97;
      gint16 *s = g_new (gint16, cnt);
      for (gsize k = 0; k < cnt; k++)
        s[k] = (gint16) ABS (j.diff[k * 97]);
      qsort (s, cnt, sizeof *s, cmp_i16);
      m = cnt ? (gsize) s[cnt / 2] : 1;
      g_free (s);
      j.sigma = MAX (1.0f, (gfloat) m / 0.6745f);
    }
    if (g_getenv ("FP6_IMAGING_DEBUG"))
      g_printerr ("night frame %d: shift %d,%d sigma %.2f\n", i, j.gdx, j.gdy,
          j.sigma);
    fp6_parallel_rows (h, nm_accumulate, &j);
    fp6_progress (prog, (gdouble) (i + 1) / n * 0.9);
  }
  /* The ISP clips noise at black, so the mean of noisy dark pixels sits
   * above black: averaging lifts and greys the shadows. Pull the darkest
   * 0.5 % back down (bounded, so a scene without blacks is not darkened
   * much). */
  if (ok) {
    guint hist[1024] = { 0 }, cnt = 0, acc = 0;
    for (gsize k = 0; k < (gsize) w * h; k += 7) {
      gfloat l = (j.acc[3 * k] + j.acc[3 * k + 1] + j.acc[3 * k + 2]) /
          (3.0f * j.wsum[k]) * j.boost;
      hist[CLAMP ((gint) (l * 1023 * 8), 0, 1023)]++;
      cnt++;
    }
    for (gint b = 0; b < 1024; b++) {
      acc += hist[b];
      if (acc * 200 >= cnt) {
        j.black = MIN (b / (1023.0f * 8), 0.01f);
        break;
      }
    }
  }
  o.j = &j;
  o.out = ok && !fp6_cancelled (prog) ? fp6_image_new (w, h) : NULL;
  if (o.out)
    fp6_parallel_rows (h, nm_output, &o);
  fp6_progress (prog, 1.0);
  for (gint i = 0; i < n; i++)
    fp6_gray_free (g[i]);
  g_free (g);
  g_free (j.tdx);
  g_free (j.tdy);
  g_free (j.diff);
  g_free (j.acc);
  g_free (j.wsum);
  return o.out;
}

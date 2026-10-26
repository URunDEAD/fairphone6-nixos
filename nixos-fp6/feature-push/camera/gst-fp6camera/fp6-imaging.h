/* SPDX-License-Identifier: LGPL-2.1-or-later
 *
 * fp6-imaging: CPU image processing for the FP6 camera's computational
 * modes (camera phase 2): exposure fusion (HDR), burst merging (night),
 * alignment, panorama stitching, document rectification.
 *
 * Plain C + GLib, no GStreamer: the offline test tool links it as well.
 * Images are 8-bit packed RGB with rows padded to 4 bytes (the layout of
 * GStreamer's RGB format). Float work buffers are interleaved. The hot
 * loops are written for the compiler's auto-vectoriser (NEON on aarch64)
 * and split over a few threads (fp6_parallel_rows); callers run them in a
 * low-priority thread so the viewfinder keeps its CPU.
 */
#ifndef FP6_IMAGING_H
#define FP6_IMAGING_H

#include <glib.h>

typedef struct
{
  gint w, h, stride;            /* stride = round_up_4 (w * 3) */
  guint8 *data;
} Fp6Image;

typedef struct
{
  gint w, h;
  guint8 *d;                    /* w * h, no padding */
} Fp6Gray;

/* progress reporting: fraction 0..1 of the whole operation */
typedef struct
{
  void (*fn) (gpointer user, gdouble fraction);
  gpointer user;
  gdouble base, span;           /* this call reports into [base, base+span] */
  const volatile gint *cancel;  /* != 0: abort as soon as possible */
} Fp6Progress;

void fp6_progress (const Fp6Progress * p, gdouble fraction);
gboolean fp6_cancelled (const Fp6Progress * p);
Fp6Progress fp6_progress_sub (const Fp6Progress * p, gdouble base,
    gdouble span);

/* threads used by the parallel loops (default 4) */
void fp6_imaging_set_threads (gint n);
/* call fn (y0, y1, user) for row bands of [0, h) on several threads */
void fp6_parallel_rows (gint h, void (*fn) (gint y0, gint y1, gpointer user),
    gpointer user);

Fp6Image *fp6_image_new (gint w, gint h);
void fp6_image_free (Fp6Image * img);
Fp6Image *fp6_image_copy (const Fp6Image * img);
/* from a packed frame: ps bytes per pixel, ro/go/bo byte offsets of R/G/B */
Fp6Image *fp6_image_from_packed (const guint8 * src, gint stride, gint w,
    gint h, gint ps, gint ro, gint go, gint bo);
/* crop (x, y, w, h), clamped to the image */
Fp6Image *fp6_image_crop (const Fp6Image * img, gint x, gint y, gint w,
    gint h);
/* rotate clockwise by 0/90/180/270 after mirroring (mirror_x) */
Fp6Image *fp6_image_rotate (const Fp6Image * img, gint rot_cw,
    gboolean mirror_x);
/* bilinear resize */
Fp6Image *fp6_image_resize (const Fp6Image * img, gint w, gint h);
gboolean fp6_image_write_ppm (const Fp6Image * img, const gchar * path);
Fp6Image *fp6_image_read_ppm (const gchar * path);

Fp6Gray *fp6_gray_new (gint w, gint h);
void fp6_gray_free (Fp6Gray * g);
/* luma, box-downsampled by 2^shift */
Fp6Gray *fp6_gray_from_image (const Fp6Image * img, gint shift);
Fp6Gray *fp6_gray_half (const Fp6Gray * g);

/* Exposure-invariant translation (Ward's median threshold bitmaps):
 * shift (dx, dy) so that img (x + dx, y + dy) matches ref (x, y).
 * max_shift: search radius in pixels. */
void fp6_align_mtb (const Fp6Image * ref, const Fp6Image * img,
    gint max_shift, gint * dx, gint * dy);
/* same for frames ev stops apart (img exposure - ref exposure): the darker
 * one is brightened (with clipping) before thresholding */
void fp6_align_mtb_ev (const Fp6Image * ref, const Fp6Image * img,
    gdouble ev, gint max_shift, gint * dx, gint * dy);

/* Exposure fusion (Mertens, Kautz, Van Reeth 2007) of n images, each
 * sampled at (x + dx[i], y + dy[i]). Contrast, saturation and
 * well-exposedness weights, Laplacian pyramid blending. */
Fp6Image *fp6_mertens (Fp6Image * const *imgs, const gint * dx,
    const gint * dy, gint n, const Fp6Progress * prog);

/* translation of img against ref (same exposure), coarse-to-fine SAD */
void fp6_align_translation (const Fp6Gray * ref, const Fp6Gray * img,
    gint max_shift, gint * dx, gint * dy);

/* Night mode: merge a burst of n similar frames. The sharpest of the
 * first frames is the reference (*ref_out); the others are aligned to it
 * (global translation, then per 64 px tile +-2 px), averaged in linear
 * light with a per-pixel robustness weight (moving things and residual
 * misalignment fall back to the reference) and multiplied by boost. */
Fp6Image *fp6_night_merge (Fp6Image * const *imgs, gint n, gdouble boost,
    gint * ref_out, const Fp6Progress * prog);

#endif

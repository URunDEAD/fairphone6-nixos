/* SPDX-License-Identifier: LGPL-2.1-or-later
 *
 * fp6-vision: C ABI of the Rust panorama / document-scan library
 * (vision/lib.rs, a rustc staticlib). Camera phase 2.
 */
#ifndef FP6_VISION_H
#define FP6_VISION_H

#include <stdint.h>

typedef struct
{
  int w, h, stride;             /* stride in bytes */
  int ps;                       /* bytes per pixel (3 or 4) */
  int ro, go, bo;               /* byte offsets of R, G, B */
  uint8_t *data;
} Fp6vImage;

typedef struct Fp6vPano Fp6vPano;

/* exif: orientation that shows the stream upright; focal_px: focal length
 * in stream pixels; scale: stream pixels per tracking-image pixel */
Fp6vPano *fp6v_pano_new (uint16_t exif, double focal_px, double scale,
    int max_keys);
void fp6v_pano_free (Fp6vPano * p);
/* 0 nothing, 1 keep this frame as a keyframe, 2 keep it, sweep complete */
int fp6v_pano_feed (Fp6vPano * p, const uint8_t * luma, int w, int h,
    int stride);
/* step: 0..1 of the way to the next keyframe; drift: vertical drift in
 * frame heights; dir: +1 sweeping right, -1 left, 0 not yet moving */
void fp6v_pano_status (const Fp6vPano * p, double *step, double *drift,
    int *dir);
int fp6v_pano_keys (const Fp6vPano * p);
/* frames in stream orientation, in feed order; out freed by fp6v_free.
 * 0 = ok */
int fp6v_pano_stitch (const Fp6vPano * p, const Fp6vImage * frames, int n,
    Fp6vImage * out, void (*progress) (void *user, double f), void *user);
void fp6v_free (Fp6vImage * img);

#endif

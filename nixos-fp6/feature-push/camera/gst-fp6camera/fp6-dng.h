/* SPDX-License-Identifier: LGPL-2.1-or-later
 *
 * fp6-dng: minimal DNG 1.4 writer for the FP6 raw (Bayer) stream
 * (camera phase 2). One uncompressed 16-bit CFA strip in IFD0 plus an
 * EXIF IFD; enough for RawTherapee, darktable, dcraw/LibRaw and Adobe.
 * Pure C (GLib only) so the offline test tool can link it too.
 */
#ifndef FP6_DNG_H
#define FP6_DNG_H

#include <glib.h>

typedef struct
{
  /* packed source: MIPI CSI-2 10/12 bit, or unpacked 16-bit little endian */
  const guint8 *data;
  gint width, height, stride;
  gint bits;                    /* 10 or 12 = CSI-2 packed, 16 = u16 LE */
  guint8 cfa[4];                /* 2x2 pattern, row major: 0 R, 1 G, 2 B */
  guint32 black, white;         /* sensor codes (e.g. 64 / 1023) */
  gdouble neutral[3];           /* AsShotNeutral (camera RGB of white) */
  /* ColorMatrix1/2 (XYZ -> camera), 0 illuminant = absent */
  gdouble cm1[9], cm2[9];
  guint16 illum1, illum2;       /* EXIF LightSource codes (17 A, 21 D65) */
  guint16 orientation;          /* TIFF/EXIF 1..8 */
  const gchar *make, *model, *unique_model, *software;
  gdouble exposure_s;           /* 0 = unknown */
  gint iso;                     /* 0 = unknown */
  gdouble focal_mm;             /* 0 = unknown */
  gchar datetime[20];           /* "YYYY:MM:DD HH:MM:SS" or "" */
} Fp6DngInfo;

/* CFA pattern from a GStreamer/libcamera Bayer order prefix ("bggr10p",
 * "SGRBG10_CSI2P", "rggb" ...); swap_rb exchanges R and B. */
gboolean fp6_dng_cfa_from_name (const gchar * name, gboolean swap_rb,
    guint8 cfa[4]);

/* EXIF orientation of: first the stream's videoflip method (sensor
 * coordinates), then a clockwise rotation (0/90/180/270) for display. */
guint16 fp6_dng_orientation (const gchar * flip_method, gint rotate_cw);

/* Write info to path (atomically via path.part). */
gboolean fp6_dng_write (const gchar * path, const Fp6DngInfo * info,
    GError ** err);

#endif

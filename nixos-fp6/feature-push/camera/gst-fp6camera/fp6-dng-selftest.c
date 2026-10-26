/* SPDX-License-Identifier: LGPL-2.1-or-later
 *
 * fp6-dng-selftest OUT.dng [bits] : write a synthetic CSI-2 packed BGGR
 * frame (red/green/blue/grey quadrants) through fp6-dng.c; the build's
 * install check decodes it with dcraw and reads the tags with exiftool.
 * Also checks the orientation and CFA helpers.
 */
#include "fp6-dng.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static void
pack10 (guint8 * dst, const guint16 * v, gint w)
{
  for (gint x = 0; x < w; x += 4, dst += 5) {
    dst[4] = 0;
    for (gint i = 0; i < 4; i++) {
      dst[i] = v[x + i] >> 2;
      dst[4] |= (v[x + i] & 3) << (2 * i);
    }
  }
}

int
main (int argc, char **argv)
{
  const gint w = 64, h = 48, stride = 96;      /* padded like CAMSS */
  guint8 *raw = g_malloc0 ((gsize) stride * h);
  guint16 row[64];
  Fp6DngInfo in = { 0 };
  GError *err = NULL;
  guint8 cfa[4];

  if (argc < 2)
    return 2;
  /* helpers */
  if (!fp6_dng_cfa_from_name ("bggr10p", FALSE, cfa) || cfa[0] != 2
      || cfa[1] != 1 || cfa[3] != 0)
    return 10;
  if (!fp6_dng_cfa_from_name ("SGRBG10_CSI2P", TRUE, cfa) || cfa[0] != 1
      || cfa[1] != 2 || cfa[2] != 0)      /* GRBG with R/B swapped = GBRG */
    return 11;
  if (fp6_dng_cfa_from_name ("bgbg", FALSE, cfa))
    return 12;
  if (fp6_dng_orientation ("none", 0) != 1
      || fp6_dng_orientation ("none", 90) != 6
      || fp6_dng_orientation ("none", 270) != 8
      || fp6_dng_orientation ("rotate-180", 90) != 8
      || fp6_dng_orientation ("rotate-180", 180) != 1
      || fp6_dng_orientation ("horizontal-flip", 0) != 2
      || fp6_dng_orientation ("horizontal-flip", 90) != 7)
    return 13;

  /* BGGR: B at (0,0), G at (1,0)/(0,1), R at (1,1). Quadrants: top-left
   * red, top-right green, bottom-left blue, bottom-right grey. */
  for (gint y = 0; y < h; y++) {
    for (gint x = 0; x < w; x++) {
      gint c = (y & 1) ? ((x & 1) ? 0 : 1) : ((x & 1) ? 1 : 2);   /* R G B */
      gint q = (y < h / 2 ? 0 : 2) + (x < w / 2 ? 0 : 1);
      gint want = q == 0 ? 0 : q == 1 ? 1 : q == 2 ? 2 : -1;
      row[x] = 64 + ((want < 0 || want == c) ? 800 : 20);
    }
    pack10 (raw + (gsize) y * stride, row, w);
  }
  in.data = raw;
  in.width = w;
  in.height = h;
  in.stride = stride;
  in.bits = 10;
  fp6_dng_cfa_from_name ("bggr10p", FALSE, in.cfa);
  in.black = 64;
  in.white = 1023;
  in.neutral[0] = in.neutral[1] = in.neutral[2] = 1.0;
  for (gint i = 0; i < 9; i++)
    in.cm1[i] = in.cm2[i] = (i % 4 == 0) ? 1.0 : 0.0;
  in.cm1[1] = -0.25;            /* a negative SRATIONAL */
  in.illum1 = 17;
  in.illum2 = 21;
  in.orientation = 6;
  in.make = "Fairphone";
  in.model = "FP6";
  in.unique_model = "Fairphone FP6 selftest";
  in.software = "fp6-dng-selftest";
  in.exposure_s = 1.0 / 120;
  in.iso = 250;
  g_strlcpy (in.datetime, "2026:10:26 12:00:00", sizeof in.datetime);
  if (!fp6_dng_write (argv[1], &in, &err)) {
    fprintf (stderr, "%s\n", err->message);
    return 1;
  }
  printf ("wrote %s\n", argv[1]);
  return 0;
}

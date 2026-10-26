/* SPDX-License-Identifier: LGPL-2.1-or-later
 *
 * fp6-dng: minimal DNG writer, see fp6-dng.h.
 *
 * Layout (little endian TIFF):
 *   header | IFD0 + its out-of-line values | EXIF IFD + values | CFA strip
 * IFD0 is the raw image itself (NewSubFileType 0, PhotometricInterpretation
 * CFA, 16 bits per sample, uncompressed, one strip).
 */
#include "fp6-dng.h"

#include <errno.h>
#include <glib/gstdio.h>
#include <math.h>
#include <stdio.h>
#include <string.h>

enum
{
  T_BYTE = 1, T_ASCII = 2, T_SHORT = 3, T_LONG = 4, T_RATIONAL = 5,
  T_UNDEFINED = 7, T_SRATIONAL = 10,
};

static const guint type_size[] = { 0, 1, 1, 2, 4, 8, 1, 1, 2, 4, 8, 4, 8 };

typedef struct
{
  guint16 tag, type;
  guint32 count;
  guint8 *data;                 /* count * type_size bytes, owned */
} Ent;

typedef struct
{
  GArray *e;                    /* Ent, kept sorted by tag */
} Ifd;

static void
ifd_init (Ifd * ifd)
{
  ifd->e = g_array_new (FALSE, TRUE, sizeof (Ent));
}

static void
ifd_clear (Ifd * ifd)
{
  for (guint i = 0; i < ifd->e->len; i++)
    g_free (g_array_index (ifd->e, Ent, i).data);
  g_array_free (ifd->e, TRUE);
}

static Ent *
ifd_add (Ifd * ifd, guint16 tag, guint16 type, guint32 count)
{
  Ent n = { tag, type, count, g_malloc0 (MAX (count * type_size[type], 4)) };
  guint i = 0;

  while (i < ifd->e->len && g_array_index (ifd->e, Ent, i).tag < tag)
    i++;
  g_array_insert_val (ifd->e, i, n);
  return &g_array_index (ifd->e, Ent, i);
}

static void
put16 (guint8 * p, guint16 v)
{
  p[0] = v & 0xff;
  p[1] = v >> 8;
}

static void
put32 (guint8 * p, guint32 v)
{
  p[0] = v & 0xff;
  p[1] = (v >> 8) & 0xff;
  p[2] = (v >> 16) & 0xff;
  p[3] = v >> 24;
}

static void
add_short (Ifd * ifd, guint16 tag, guint n, const guint16 * v)
{
  Ent *e = ifd_add (ifd, tag, T_SHORT, n);
  for (guint i = 0; i < n; i++)
    put16 (e->data + 2 * i, v[i]);
}

static void
add_short1 (Ifd * ifd, guint16 tag, guint16 v)
{
  add_short (ifd, tag, 1, &v);
}

static void
add_long1 (Ifd * ifd, guint16 tag, guint32 v)
{
  put32 (ifd_add (ifd, tag, T_LONG, 1)->data, v);
}

static void
add_bytes (Ifd * ifd, guint16 tag, guint16 type, guint n, const guint8 * v)
{
  memcpy (ifd_add (ifd, tag, type, n)->data, v, n);
}

static void
add_ascii (Ifd * ifd, guint16 tag, const gchar * s)
{
  if (s && *s)
    add_bytes (ifd, tag, T_ASCII, strlen (s) + 1, (const guint8 *) s);
}

static void
add_rational (Ifd * ifd, guint16 tag, gboolean sign, guint n,
    const gdouble * v)
{
  Ent *e = ifd_add (ifd, tag, sign ? T_SRATIONAL : T_RATIONAL, n);
  for (guint i = 0; i < n; i++) {
    gdouble x = v[i];
    gint64 den = 1000000, num;
    /* keep num within 32 bits */
    while (den > 1 && fabs (x * den) > 2.0e9)
      den /= 10;
    num = (gint64) llround (x * den);
    put32 (e->data + 8 * i, (guint32) (gint32) num);
    put32 (e->data + 8 * i + 4, (guint32) den);
  }
}

static Ent *
ifd_find (Ifd * ifd, guint16 tag)
{
  for (guint i = 0; i < ifd->e->len; i++)
    if (g_array_index (ifd->e, Ent, i).tag == tag)
      return &g_array_index (ifd->e, Ent, i);
  return NULL;
}

/* bytes of the IFD incl. out-of-line values (word aligned) */
static gsize
ifd_size (Ifd * ifd)
{
  gsize n = 2 + 12 * ifd->e->len + 4;
  for (guint i = 0; i < ifd->e->len; i++) {
    Ent *e = &g_array_index (ifd->e, Ent, i);
    gsize len = (gsize) e->count * type_size[e->type];
    if (len > 4)
      n += (len + 1) & ~(gsize) 1;
  }
  return n;
}

static void
ifd_serialise (Ifd * ifd, guint32 base, GByteArray * out)
{
  gsize start = out->len;
  guint32 data_off = base + 2 + 12 * ifd->e->len + 4;
  guint8 b[12];

  put16 (b, ifd->e->len);
  g_byte_array_append (out, b, 2);
  for (guint i = 0; i < ifd->e->len; i++) {
    Ent *e = &g_array_index (ifd->e, Ent, i);
    gsize len = (gsize) e->count * type_size[e->type];
    put16 (b, e->tag);
    put16 (b + 2, e->type);
    put32 (b + 4, e->count);
    if (len <= 4) {
      memset (b + 8, 0, 4);
      memcpy (b + 8, e->data, len);
    } else {
      put32 (b + 8, data_off);
      data_off += (len + 1) & ~(gsize) 1;
    }
    g_byte_array_append (out, b, 12);
  }
  memset (b, 0, 4);             /* no next IFD */
  g_byte_array_append (out, b, 4);
  for (guint i = 0; i < ifd->e->len; i++) {
    Ent *e = &g_array_index (ifd->e, Ent, i);
    gsize len = (gsize) e->count * type_size[e->type];
    if (len > 4) {
      g_byte_array_append (out, e->data, len);
      if (len & 1)
        g_byte_array_append (out, b, 1);
    }
  }
  g_assert (out->len - start == ifd_size (ifd));
}

gboolean
fp6_dng_cfa_from_name (const gchar * name, gboolean swap_rb, guint8 cfa[4])
{
  const gchar *p = name;

  if (!p)
    return FALSE;
  if ((p[0] == 'S' || p[0] == 's') && strlen (p) >= 5
      && strchr ("RGBrgb", p[1]))
    p++;                        /* libcamera "SBGGR10..." */
  for (gint i = 0; i < 4; i++) {
    switch (g_ascii_tolower (p[i])) {
      case 'r':
        cfa[i] = swap_rb ? 2 : 0;
        break;
      case 'g':
        cfa[i] = 1;
        break;
      case 'b':
        cfa[i] = swap_rb ? 0 : 2;
        break;
      default:
        return FALSE;
    }
  }
  /* two greens on a diagonal, one red and one blue on the other */
  if (cfa[0] == 1 && cfa[3] == 1)
    return cfa[1] + cfa[2] == 2 && cfa[1] != 1;
  if (cfa[1] == 1 && cfa[2] == 1)
    return cfa[0] + cfa[3] == 2 && cfa[0] != 1;
  return FALSE;
}

/* 2x2 integer transforms on (x right, y down) */
typedef struct
{
  gint a, b, c, d;              /* x' = a x + b y ; y' = c x + d y */
} Xf;

static Xf
xf_mul (Xf p, Xf q)             /* p after q */
{
  Xf r = { p.a * q.a + p.b * q.c, p.a * q.b + p.b * q.d,
    p.c * q.a + p.d * q.c, p.c * q.b + p.d * q.d
  };
  return r;
}

guint16
fp6_dng_orientation (const gchar * flip, gint rotate_cw)
{
  /* EXIF orientation n: the operation that displays the stored image */
  static const Xf exif[9] = {
    {0, 0, 0, 0},
    {1, 0, 0, 1},               /* 1 identity */
    {-1, 0, 0, 1},              /* 2 mirror horizontal */
    {-1, 0, 0, -1},             /* 3 rotate 180 */
    {1, 0, 0, -1},              /* 4 mirror vertical */
    {0, 1, 1, 0},               /* 5 transpose */
    {0, -1, 1, 0},              /* 6 rotate 90 cw */
    {0, -1, -1, 0},             /* 7 transverse */
    {0, 1, -1, 0},              /* 8 rotate 270 cw */
  };
  Xf f = exif[1], r = exif[1], t;

  if (!g_strcmp0 (flip, "horizontal-flip"))
    f = exif[2];
  else if (!g_strcmp0 (flip, "vertical-flip"))
    f = exif[4];
  else if (!g_strcmp0 (flip, "rotate-180"))
    f = exif[3];
  else if (!g_strcmp0 (flip, "clockwise"))
    f = exif[6];
  else if (!g_strcmp0 (flip, "counterclockwise"))
    f = exif[8];
  switch (((rotate_cw % 360) + 360) % 360) {
    case 90:
      r = exif[6];
      break;
    case 180:
      r = exif[3];
      break;
    case 270:
      r = exif[8];
      break;
  }
  t = xf_mul (r, f);
  for (guint16 n = 1; n <= 8; n++)
    if (!memcmp (&t, &exif[n], sizeof t))
      return n;
  return 1;
}

static void
unpack_row (const guint8 * src, guint16 * dst, gint w, gint bits)
{
  gint x = 0;

  if (bits == 10) {
    for (; x + 3 < w; x += 4, src += 5) {
      guint8 l = src[4];
      dst[x] = (src[0] << 2) | (l & 3);
      dst[x + 1] = (src[1] << 2) | ((l >> 2) & 3);
      dst[x + 2] = (src[2] << 2) | ((l >> 4) & 3);
      dst[x + 3] = (src[3] << 2) | (l >> 6);
    }
    for (gint i = 0; x < w; x++, i++)   /* partial group */
      dst[x] = (src[i] << 2) | ((src[4] >> (2 * i)) & 3);
  } else if (bits == 12) {
    for (; x + 1 < w; x += 2, src += 3) {
      dst[x] = (src[0] << 4) | (src[2] & 0xf);
      dst[x + 1] = (src[1] << 4) | (src[2] >> 4);
    }
    if (x < w)
      dst[x] = (src[0] << 4) | (src[2] & 0xf);
  } else {
    for (; x < w; x++)
      dst[x] = src[2 * x] | (src[2 * x + 1] << 8);
  }
}

gboolean
fp6_dng_write (const gchar * path, const Fp6DngInfo * in, GError ** err)
{
  Ifd ifd0, exif;
  GByteArray *hdr;
  gsize size0, size1;
  guint32 exif_off, strip_off, strip_len;
  gchar *tmp;
  FILE *f;
  guint16 *row;
  gboolean ok = FALSE;
  gsize need;

  need = in->bits == 10 ? ((gsize) in->width * 5 + 3) / 4
      : in->bits == 12 ? ((gsize) in->width * 3 + 1) / 2
      : (gsize) in->width * 2;
  if (!in->data || in->width < 2 || in->height < 2 || (gsize) in->stride < need
      || (in->bits != 10 && in->bits != 12 && in->bits != 16)) {
    g_set_error (err, G_FILE_ERROR, G_FILE_ERROR_INVAL,
        "unsupported raw layout %dx%d stride %d, %d bit", in->width,
        in->height, in->stride, in->bits);
    return FALSE;
  }
  strip_len = (guint32) in->width * in->height * 2;

  ifd_init (&ifd0);
  ifd_init (&exif);

  add_long1 (&ifd0, 254, 0);    /* NewSubFileType: main image */
  add_long1 (&ifd0, 256, in->width);
  add_long1 (&ifd0, 257, in->height);
  add_short1 (&ifd0, 258, 16);
  add_short1 (&ifd0, 259, 1);   /* uncompressed */
  add_short1 (&ifd0, 262, 32803);       /* CFA */
  add_ascii (&ifd0, 271, in->make);
  add_ascii (&ifd0, 272, in->model);
  add_long1 (&ifd0, 273, 0);    /* StripOffsets, set below */
  add_short1 (&ifd0, 274, in->orientation >= 1 && in->orientation <= 8 ?
      in->orientation : 1);
  add_short1 (&ifd0, 277, 1);
  add_long1 (&ifd0, 278, in->height);
  add_long1 (&ifd0, 279, strip_len);
  add_short1 (&ifd0, 284, 1);
  add_ascii (&ifd0, 305, in->software);
  if (in->datetime[0])
    add_ascii (&ifd0, 306, in->datetime);
  {
    const guint16 dim[2] = { 2, 2 };
    add_short (&ifd0, 33421, 2, dim);   /* CFARepeatPatternDim */
    add_bytes (&ifd0, 33422, T_BYTE, 4, in->cfa);       /* CFAPattern */
  }
  if (in->exposure_s > 0)
    add_rational (&ifd0, 33434, FALSE, 1, &in->exposure_s);
  add_long1 (&ifd0, 34665, 0);  /* ExifIFD, set below */
  if (in->iso > 0)
    add_short1 (&ifd0, 34855, (guint16) MIN (in->iso, 65535));
  {
    const guint8 ver[4] = { 1, 4, 0, 0 }, back[4] = { 1, 1, 0, 0 };
    const guint8 plane[3] = { 0, 1, 2 };
    const guint16 bdim[2] = { 1, 1 };
    add_bytes (&ifd0, 50706, T_BYTE, 4, ver);
    add_bytes (&ifd0, 50707, T_BYTE, 4, back);
    add_ascii (&ifd0, 50708, in->unique_model ? in->unique_model : in->model);
    add_bytes (&ifd0, 50710, T_BYTE, 3, plane); /* CFAPlaneColor */
    add_short1 (&ifd0, 50711, 1);       /* CFALayout rectangular */
    add_short (&ifd0, 50713, 2, bdim);  /* BlackLevelRepeatDim */
    add_long1 (&ifd0, 50714, in->black);
    add_long1 (&ifd0, 50717, in->white);
  }
  if (in->illum1) {
    add_rational (&ifd0, 50721, TRUE, 9, in->cm1);
    add_short1 (&ifd0, 50778, in->illum1);
  }
  if (in->illum2) {
    add_rational (&ifd0, 50722, TRUE, 9, in->cm2);
    add_short1 (&ifd0, 50779, in->illum2);
  }
  add_rational (&ifd0, 50728, FALSE, 3, in->neutral);   /* AsShotNeutral */

  /* EXIF IFD */
  if (in->exposure_s > 0)
    add_rational (&exif, 33434, FALSE, 1, &in->exposure_s);
  if (in->iso > 0)
    add_short1 (&exif, 34855, (guint16) MIN (in->iso, 65535));
  add_bytes (&exif, 36864, T_UNDEFINED, 4, (const guint8 *) "0230");
  if (in->datetime[0])
    add_ascii (&exif, 36867, in->datetime);
  if (in->focal_mm > 0)
    add_rational (&exif, 37386, FALSE, 1, &in->focal_mm);

  size0 = ifd_size (&ifd0);
  exif_off = 8 + size0;
  size1 = ifd_size (&exif);
  strip_off = (exif_off + size1 + 15) & ~15u;
  put32 (ifd_find (&ifd0, 273)->data, strip_off);
  put32 (ifd_find (&ifd0, 34665)->data, exif_off);

  hdr = g_byte_array_sized_new (strip_off);
  g_byte_array_append (hdr, (const guint8 *) "II*\0\x08\0\0\0", 8);
  ifd_serialise (&ifd0, 8, hdr);
  ifd_serialise (&exif, exif_off, hdr);
  while (hdr->len < strip_off) {
    const guint8 z = 0;
    g_byte_array_append (hdr, &z, 1);
  }

  tmp = g_strdup_printf ("%s.part", path);
  f = g_fopen (tmp, "wb");
  if (!f) {
    g_set_error (err, G_FILE_ERROR, g_file_error_from_errno (errno),
        "cannot create %s: %s", tmp, g_strerror (errno));
    goto out;
  }
  row = g_new (guint16, in->width);
  ok = fwrite (hdr->data, 1, hdr->len, f) == hdr->len;
  for (gint y = 0; ok && y < in->height; y++) {
    unpack_row (in->data + (gsize) y * in->stride, row, in->width, in->bits);
#if G_BYTE_ORDER == G_BIG_ENDIAN
    for (gint x = 0; x < in->width; x++)
      row[x] = GUINT16_TO_LE (row[x]);
#endif
    ok = fwrite (row, 2, in->width, f) == (gsize) in->width;
  }
  g_free (row);
  if (fclose (f) != 0)
    ok = FALSE;
  if (!ok)
    g_set_error (err, G_FILE_ERROR, g_file_error_from_errno (errno),
        "cannot write %s: %s", tmp, g_strerror (errno));
  if (ok && g_rename (tmp, path) != 0) {
    g_set_error (err, G_FILE_ERROR, g_file_error_from_errno (errno),
        "cannot rename %s: %s", tmp, g_strerror (errno));
    ok = FALSE;
  }
  if (!ok)
    g_unlink (tmp);
out:
  g_free (tmp);
  g_byte_array_unref (hdr);
  ifd_clear (&ifd0);
  ifd_clear (&exif);
  return ok;
}

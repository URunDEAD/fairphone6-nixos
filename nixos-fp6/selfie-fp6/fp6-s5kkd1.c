// SPDX-License-Identifier: GPL-2.0-only
/* Experimental, unintegrated FP6 S5KKD1SP source candidate.
 * Register facts: exact FP6 public firmware, source-manifest.json.
 * API/lifecycle reference: upstream GPL-2.0 s5kjn1.c (Linaro, 2025).
 * No generic Samsung mode table is used. Firmware is separate, hash-pinned.
 */
#include <crypto/sha2.h>
#include <linux/clk.h>
#include <linux/delay.h>
#include <linux/firmware.h>
#include <linux/gpio/consumer.h>
#include <linux/i2c.h>
#include <linux/module.h>
#include <linux/pm_runtime.h>
#include <linux/regulator/consumer.h>
#include <linux/unaligned.h>
#include <media/v4l2-cci.h>
#include <media/v4l2-ctrls.h>
#include <media/v4l2-device.h>
#include <media/v4l2-fwnode.h>
#include "fp6-s5kkd1-firmware.h"

#define KD1_WIDTH 3104
#define KD1_HEIGHT 2336
#define KD1_HTS 3640
#define KD1_VTS 5078
#define KD1_VT_CLOCK 560000000
#define KD1_MCLK 24000000
#define KD1_LINK_FREQ 906000000LL
#define KD1_FW "fp6-s5kkd1-v1.bin"
/* Diagnostic candidate: no stream/init writes unless explicitly opted in. */
static bool enable_stream;
module_param(enable_stream, bool, 0400);
MODULE_PARM_DESC(enable_stream, "Allow reviewed experimental streaming after strict ID probe");

struct kd1 {
 struct device *dev;
 struct regmap *regmap;
 struct clk *mclk;
 struct gpio_desc *reset;
 struct regulator *vio, *vdig, *vana;
 struct v4l2_subdev sd;
 struct media_pad pad;
 struct mutex lock;
 struct v4l2_ctrl_handler ctrls;
 struct v4l2_ctrl *exposure, *gain, *vblank;
 bool streaming;
};
static const s64 kd1_link_freqs[] = { KD1_LINK_FREQ };
static const char * const kd1_patterns[] = { "Disabled", "Color Bars" };
static inline struct kd1 *to_kd1(struct v4l2_subdev *sd)
{ return container_of(sd, struct kd1, sd); }
static int kd1_id(struct kd1 *s)
{
 u64 id;
 int ret = cci_read(s->regmap, CCI_REG16(0x0000), &id, NULL);
 if (ret) return ret;
 if (id != 0x4841)
  return dev_err_probe(s->dev, -ENODEV, "ID mismatch: %llx expected4841; no mode writes\n", id);
 return 0;
}
static int kd1_power_off(struct device *dev)
{
 struct kd1 *s = to_kd1(dev_get_drvdata(dev));
 clk_disable_unprepare(s->mclk);
 usleep_range(1000, 1500);
 gpiod_set_value_cansleep(s->reset, 1);
 usleep_range(1000, 1500);
 regulator_disable(s->vana);
 regulator_disable(s->vdig);
 regulator_disable(s->vio);
 return 0;
}
static int kd1_power_on(struct device *dev)
{
 struct kd1 *s = to_kd1(dev_get_drvdata(dev));
 int ret;
 /* GPIO is ACTIVE_LOW: logical1 asserts physical RESET_N low. */
 gpiod_set_value_cansleep(s->reset, 1);
 usleep_range(2000, 2500);
 ret = regulator_enable(s->vio);
 if (ret) return ret;
 usleep_range(2000, 2500);
 ret = regulator_enable(s->vdig);
 if (ret) goto off_vio;
 usleep_range(2000, 2500);
 ret = regulator_enable(s->vana);
 if (ret) goto off_vdig;
 usleep_range(2000, 2500);
 gpiod_set_value_cansleep(s->reset, 0);
 usleep_range(2000, 2500);
 ret = clk_prepare_enable(s->mclk);
 if (ret) goto assert_reset;
 usleep_range(2000, 2500);
 if (clk_get_rate(s->mclk) != KD1_MCLK) { ret = -EINVAL; goto off_clock; }
 /* Recheck on every power cycle, BEFORE initialization or register writes. */
 ret = kd1_id(s);
 if (!ret) return 0;
off_clock:
 clk_disable_unprepare(s->mclk);
assert_reset:
 gpiod_set_value_cansleep(s->reset, 1);
 regulator_disable(s->vana);
off_vdig:
 regulator_disable(s->vdig);
off_vio:
 regulator_disable(s->vio);
 return ret;
}
static int kd1_write_record(struct kd1 *s, const u8 *p)
{
 u16 addr = get_unaligned_be16(p), value = get_unaligned_be16(p + 2);
 u32 delay = get_unaligned_be32(p + 4);
 int ret;
 if (delay > 10000) return -EINVAL;
 ret = cci_write(s->regmap, CCI_REG16(addr), value, NULL);
 if (!ret && delay) usleep_range(delay, delay + 500);
 return ret;
}
static int kd1_load_mode(struct kd1 *s)
{
 const struct firmware *fw;
 u8 digest[SHA256_DIGEST_SIZE];
 unsigned int i;
 int ret = request_firmware(&fw, KD1_FW, s->dev);
 if (ret) return ret;
 if (fw->size != KD1_FW_SIZE || memcmp(fw->data, "FP6KD1\0\1", 8) ||
     get_unaligned_be32(fw->data + 8) != 258 ||
     get_unaligned_be32(fw->data + 12) != 237) { ret = -EINVAL; goto out; }
 sha256(fw->data, fw->size, digest);
 if (memcmp(digest, kd1_fw_sha256, sizeof(digest))) { ret = -EBADMSG; goto out; }
 /* Hash validates the whole list BEFORE the first initialization write. */
 for (i = 0; i < 258 + 237; i++) {
  ret = kd1_write_record(s, fw->data + 16 + 8 * i);
  if (ret) break;
 }
out:
 release_firmware(fw);
 return ret;
}
static int kd1_stop(struct kd1 *s)
{
 u64 status;
 unsigned int i;
 int ret = cci_write(s->regmap, CCI_REG16(0x0100), 0, NULL);
 if (ret) return ret;
 /* Exact vendor stop condition: frame counter8-bit becomesff; bound50ms. */
 for (i = 0; i < 50; i++) {
  ret = cci_read(s->regmap, CCI_REG8(0x0005), &status, NULL);
  if (ret || status == 0xff) return ret;
  usleep_range(1000, 1100);
 }
 return -ETIMEDOUT;
}
static int kd1_ctrl(struct v4l2_ctrl *ctrl)
{
 struct kd1 *s = container_of(ctrl->handler, struct kd1, ctrls);
 int ret = 0, release;
 if (ctrl->id == V4L2_CID_VBLANK)
  __v4l2_ctrl_modify_range(s->exposure, 6,
   min_t(int, KD1_HEIGHT + ctrl->val - 11, 65515) & ~1, 2, 976);
 if (!s->streaming) return 0;
 ret = cci_write(s->regmap, CCI_REG16(0x0104), 0x0100, NULL);
 if (ret) return ret;
 switch (ctrl->id) {
 case V4L2_CID_EXPOSURE:
  cci_write(s->regmap, CCI_REG16(0x0202), ctrl->val, &ret); break;
 case V4L2_CID_ANALOGUE_GAIN:
  cci_write(s->regmap, CCI_REG16(0x0204), ctrl->val, &ret); break;
 case V4L2_CID_TEST_PATTERN:
  cci_write(s->regmap, CCI_REG16(0x0600), ctrl->val, &ret);
  cci_write(s->regmap, CCI_REG16(0x0602), 0, &ret);
  cci_write(s->regmap, CCI_REG16(0x0604), 0, &ret);
  cci_write(s->regmap, CCI_REG16(0x0606), 0, &ret);
  cci_write(s->regmap, CCI_REG16(0x0608), 0, &ret); break;
 case V4L2_CID_VBLANK:
  cci_write(s->regmap, CCI_REG16(0x0340), KD1_HEIGHT + ctrl->val, &ret); break;
 default: ret = -EINVAL;
 }
 /* Release group hold even after an earlier I2C error; preserve that error. */
 release = cci_write(s->regmap, CCI_REG16(0x0104), 0, NULL);
 return ret ?: release;
}
static const struct v4l2_ctrl_ops kd1_ctrl_ops = { .s_ctrl = kd1_ctrl };
static int kd1_enable_streams(struct v4l2_subdev *sd,
 struct v4l2_subdev_state *state, u32 pad, u64 streams_mask)
{
 struct kd1 *s = to_kd1(sd);
 int ret;
 if (!enable_stream) return -EACCES;
 ret = pm_runtime_resume_and_get(s->dev);
 if (ret) return ret;
 ret = kd1_load_mode(s);
 if (ret) goto fail;
 s->streaming = true;
 ret = __v4l2_ctrl_handler_setup(&s->ctrls);
 if (ret) goto fail;
 /* Exact16-bit vendor write: stream1 AND adjacent H/V flip register03. */
 ret = cci_write(s->regmap, CCI_REG16(0x0100), 0x0103, NULL);
 if (!ret) return 0;
fail:
 s->streaming = false;
 kd1_stop(s);
 pm_runtime_put_autosuspend(s->dev);
 return ret;
}
static int kd1_disable_streams(struct v4l2_subdev *sd,
 struct v4l2_subdev_state *state, u32 pad, u64 streams_mask)
{
 struct kd1 *s = to_kd1(sd);
 int ret = kd1_stop(s);
 s->streaming = false;
 pm_runtime_put_autosuspend(s->dev);
 return ret;
}
static void kd1_format(struct v4l2_mbus_framefmt *f)
{
 *f = (struct v4l2_mbus_framefmt) {
  .width = KD1_WIDTH, .height = KD1_HEIGHT,
  /* Vendor CFA numeric1; GRBG interpretation requires physical color check. */
  .code = MEDIA_BUS_FMT_SGRBG10_1X10, .field = V4L2_FIELD_NONE,
  .colorspace = V4L2_COLORSPACE_RAW, .xfer_func = V4L2_XFER_FUNC_NONE,
  .quantization = V4L2_QUANTIZATION_FULL_RANGE,
 };
}
static int kd1_set_fmt(struct v4l2_subdev *sd, struct v4l2_subdev_state *state,
 struct v4l2_subdev_format *fmt)
{
 if (fmt->pad) return -EINVAL;
 if (fmt->which == V4L2_SUBDEV_FORMAT_ACTIVE && to_kd1(sd)->streaming) return -EBUSY;
 kd1_format(&fmt->format);
 *v4l2_subdev_state_get_format(state, 0) = fmt->format;
 return 0;
}
static int kd1_init_state(struct v4l2_subdev *sd, struct v4l2_subdev_state *state)
{
 kd1_format(v4l2_subdev_state_get_format(state, 0));
 return 0;
}
static int kd1_enum_code(struct v4l2_subdev *sd, struct v4l2_subdev_state *state,
 struct v4l2_subdev_mbus_code_enum *code)
{
 if (code->pad || code->index) return -EINVAL;
 code->code = MEDIA_BUS_FMT_SGRBG10_1X10;
 return 0;
}
static int kd1_enum_size(struct v4l2_subdev *sd, struct v4l2_subdev_state *state,
 struct v4l2_subdev_frame_size_enum *size)
{
 if (size->pad || size->index || size->code != MEDIA_BUS_FMT_SGRBG10_1X10) return -EINVAL;
 size->min_width = size->max_width = KD1_WIDTH;
 size->min_height = size->max_height = KD1_HEIGHT;
 return 0;
}
static int kd1_selection(struct v4l2_subdev *sd, struct v4l2_subdev_state *state,
 struct v4l2_subdev_selection *sel)
{
 if (sel->pad) return -EINVAL;
 switch (sel->target) {
 case V4L2_SEL_TGT_NATIVE_SIZE:
  sel->r = (struct v4l2_rect) { 0, 0, 6560, 4928 }; return 0;
 case V4L2_SEL_TGT_CROP_BOUNDS:
 case V4L2_SEL_TGT_CROP_DEFAULT:
 case V4L2_SEL_TGT_CROP:
  /* 2x binning of the source6208x4672 centered capture rectangle. */
  sel->r = (struct v4l2_rect) { 176, 128, 6208, 4672 }; return 0;
 default: return -EINVAL;
 }
}
static int kd1_get_interval(struct v4l2_subdev *sd, struct v4l2_subdev_state *state,
 struct v4l2_subdev_frame_interval *interval)
{
 if (interval->pad) return -EINVAL;
 interval->interval.numerator = KD1_HTS * (KD1_HEIGHT + to_kd1(sd)->vblank->val);
 interval->interval.denominator = KD1_VT_CLOCK;
 return 0;
}
static int kd1_enum_interval(struct v4l2_subdev *sd, struct v4l2_subdev_state *state,
 struct v4l2_subdev_frame_interval_enum *interval)
{
 if (interval->pad || interval->index || interval->width != KD1_WIDTH ||
     interval->height != KD1_HEIGHT || interval->code != MEDIA_BUS_FMT_SGRBG10_1X10)
  return -EINVAL;
 interval->interval.numerator = KD1_HTS * KD1_VTS;
 interval->interval.denominator = KD1_VT_CLOCK;
 return 0;
}
static const struct v4l2_subdev_video_ops kd1_video_ops = { .s_stream = v4l2_subdev_s_stream_helper };
static const struct v4l2_subdev_pad_ops kd1_pad_ops = {
 .set_fmt = kd1_set_fmt, .get_fmt = v4l2_subdev_get_fmt,
 .enum_mbus_code = kd1_enum_code, .enum_frame_size = kd1_enum_size,
 .get_selection = kd1_selection,
 .get_frame_interval = kd1_get_interval, .enum_frame_interval = kd1_enum_interval,
 .enable_streams = kd1_enable_streams, .disable_streams = kd1_disable_streams,
};
static const struct v4l2_subdev_ops kd1_ops = { .video = &kd1_video_ops, .pad = &kd1_pad_ops };
static const struct v4l2_subdev_internal_ops kd1_internal_ops = { .init_state = kd1_init_state };
static const struct media_entity_operations kd1_entity_ops = { .link_validate = v4l2_subdev_link_validate };
static int kd1_hwcfg(struct kd1 *s)
{
 struct fwnode_handle *ep;
 struct v4l2_fwnode_endpoint bus = { .bus_type = V4L2_MBUS_CSI2_DPHY };
 int ret;
 ep = fwnode_graph_get_next_endpoint(dev_fwnode(s->dev), NULL);
 if (!ep) return -EINVAL;
 ret = v4l2_fwnode_endpoint_alloc_parse(ep, &bus);
 fwnode_handle_put(ep);
 if (ret) return ret;
 if (bus.bus.mipi_csi2.num_data_lanes != 4 || bus.nr_of_link_frequencies != 1 ||
     bus.link_frequencies[0] != KD1_LINK_FREQ) ret = -EINVAL;
 v4l2_fwnode_endpoint_free(&bus);
 return ret;
}
static int kd1_probe(struct i2c_client *client)
{
 struct kd1 *s;
 struct v4l2_ctrl *c;
 struct v4l2_fwnode_device_properties props;
 int ret;
 /* Fail closed on a DT address copied from the donor/EEPROM. */
 if (client->addr != 0x3d) return -EINVAL;
 s = devm_kzalloc(&client->dev, sizeof(*s), GFP_KERNEL);
 if (!s) return -ENOMEM;
 s->dev = &client->dev;
 mutex_init(&s->lock);
 v4l2_i2c_subdev_init(&s->sd, client, &kd1_ops);
 s->regmap = devm_cci_regmap_init_i2c(client, 16);
 if (IS_ERR(s->regmap)) return PTR_ERR(s->regmap);
 s->mclk = devm_v4l2_sensor_clk_get(s->dev, NULL);
 if (IS_ERR(s->mclk)) return PTR_ERR(s->mclk);
 if (clk_get_rate(s->mclk) != KD1_MCLK) return -EINVAL;
 ret = kd1_hwcfg(s);
 if (ret) return ret;
 s->reset = devm_gpiod_get(s->dev, "reset", GPIOD_OUT_HIGH);
 if (IS_ERR(s->reset)) return PTR_ERR(s->reset);
 s->vio = devm_regulator_get(s->dev, "vddio");
 if (IS_ERR(s->vio)) return PTR_ERR(s->vio);
 s->vdig = devm_regulator_get(s->dev, "vddd");
 if (IS_ERR(s->vdig)) return PTR_ERR(s->vdig);
 s->vana = devm_regulator_get(s->dev, "vdda");
 if (IS_ERR(s->vana)) return PTR_ERR(s->vana);
 ret = kd1_power_on(s->dev);
 if (ret) return ret;
 v4l2_ctrl_handler_init(&s->ctrls, 9);
 s->ctrls.lock = &s->lock;
 c = v4l2_ctrl_new_int_menu(&s->ctrls, NULL, V4L2_CID_LINK_FREQ, 0, 0, kd1_link_freqs);
 if (c) c->flags |= V4L2_CTRL_FLAG_READ_ONLY;
 c = v4l2_ctrl_new_std(&s->ctrls, NULL, V4L2_CID_PIXEL_RATE, KD1_VT_CLOCK, KD1_VT_CLOCK, 1, KD1_VT_CLOCK);
 if (c) c->flags |= V4L2_CTRL_FLAG_READ_ONLY;
 c = v4l2_ctrl_new_std(&s->ctrls, NULL, V4L2_CID_HBLANK, KD1_HTS-KD1_WIDTH, KD1_HTS-KD1_WIDTH, 1, KD1_HTS-KD1_WIDTH);
 if (c) c->flags |= V4L2_CTRL_FLAG_READ_ONLY;
 s->vblank = v4l2_ctrl_new_std(&s->ctrls, &kd1_ctrl_ops, V4L2_CID_VBLANK, KD1_VTS-KD1_HEIGHT, 65535-KD1_HEIGHT, 1, KD1_VTS-KD1_HEIGHT);
 s->exposure = v4l2_ctrl_new_std(&s->ctrls, &kd1_ctrl_ops, V4L2_CID_EXPOSURE, 6, (KD1_VTS-11) & ~1, 2, 976);
 s->gain = v4l2_ctrl_new_std(&s->ctrls, &kd1_ctrl_ops, V4L2_CID_ANALOGUE_GAIN, 32, 2048, 1, 128);
 v4l2_ctrl_new_std_menu_items(&s->ctrls, &kd1_ctrl_ops, V4L2_CID_TEST_PATTERN, 1, 0, 0, kd1_patterns);
 ret = s->ctrls.error;
 if (ret) goto free_ctrls;
 /* Publish only properties actually supplied by DT: no guessed rotation. */
 ret = v4l2_fwnode_device_parse(s->dev, &props);
 if (ret) goto free_ctrls;
 ret = v4l2_ctrl_new_fwnode_properties(&s->ctrls, &kd1_ctrl_ops, &props);
 if (ret) goto free_ctrls;
 s->sd.ctrl_handler = &s->ctrls;
 s->sd.state_lock = &s->lock;
 s->sd.internal_ops = &kd1_internal_ops;
 s->sd.flags |= V4L2_SUBDEV_FL_HAS_DEVNODE;
 s->sd.entity.ops = &kd1_entity_ops;
 s->sd.entity.function = MEDIA_ENT_F_CAM_SENSOR;
 s->pad.flags = MEDIA_PAD_FL_SOURCE;
 ret = media_entity_pads_init(&s->sd.entity, 1, &s->pad);
 if (ret) goto free_ctrls;
 ret = v4l2_subdev_init_finalize(&s->sd);
 if (ret) goto free_entity;
 pm_runtime_set_active(s->dev);
 pm_runtime_enable(s->dev);
 ret = v4l2_async_register_subdev_sensor(&s->sd);
 if (ret) goto cleanup_sd;
 pm_runtime_set_autosuspend_delay(s->dev, 1000);
 pm_runtime_use_autosuspend(s->dev);
 pm_runtime_idle(s->dev);
 dev_info(s->dev, "ID4841 verified; stream opt-in=%d; unvalidated906MHz candidate\n", enable_stream);
 return 0;
cleanup_sd:
 pm_runtime_disable(s->dev);
 pm_runtime_set_suspended(s->dev);
 v4l2_subdev_cleanup(&s->sd);
free_entity:
 media_entity_cleanup(&s->sd.entity);
free_ctrls:
 v4l2_ctrl_handler_free(&s->ctrls);
 kd1_power_off(s->dev);
 return ret;
}
static void kd1_remove(struct i2c_client *client)
{
 struct v4l2_subdev *sd = i2c_get_clientdata(client);
 struct kd1 *s = to_kd1(sd);
 v4l2_async_unregister_subdev(sd);
 if (s->streaming) { kd1_stop(s); s->streaming = false; pm_runtime_put_noidle(s->dev); }
 pm_runtime_disable(s->dev);
 if (!pm_runtime_status_suspended(s->dev)) kd1_power_off(s->dev);
 pm_runtime_set_suspended(s->dev);
 v4l2_subdev_cleanup(sd);
 media_entity_cleanup(&sd->entity);
 v4l2_ctrl_handler_free(&s->ctrls);
 mutex_destroy(&s->lock);
}
static const struct dev_pm_ops kd1_pm = { SET_RUNTIME_PM_OPS(kd1_power_off, kd1_power_on, NULL) };
static const struct of_device_id kd1_match[] = { { .compatible = "samsung,s5kkd1" }, {} };
MODULE_DEVICE_TABLE(of, kd1_match);
static struct i2c_driver kd1_driver = {
 .driver = { .name = "fp6-s5kkd1", .of_match_table = kd1_match, .pm = &kd1_pm },
 .probe = kd1_probe, .remove = kd1_remove,
};
module_i2c_driver(kd1_driver);
MODULE_FIRMWARE(KD1_FW);
MODULE_DESCRIPTION("Unintegrated FP6 S5KKD1SP strict-ID diagnostic candidate");
MODULE_LICENSE("GPL");
MODULE_VERSION("0.2-fwnode-controls");

# Fairphone 6 kernel patches: mail-ready series

> **Review before sending. Nothing has been sent.**
> These patches were written with an AI coding assistant. Following the
> kernel's `Documentation/process/coding-assistants.rst`, the commits carry
> `Assisted-by: Claude:claude-opus-5-5` and **no `Signed-off-by`**: only a
> human can certify the Developer Certificate of Origin
> (<https://developercertificate.org>). Read every patch. If you agree to
> the DCO for it, regenerate the series with your sign-off:
>
>     SIGNOFF=1 ./make-series.sh
>
> (`git format-patch --signoff` adds `Signed-off-by: Paul Schuldesz
> <you@example.com>` from `git config user.*` of each repo.)
> The `Co-Authored-By:` trailer used in the other FP6 repos is **not** used
> here: the kernel's documented form is `Assisted-by:`.

## Layout

| path | what |
|---|---|
| `mainline/` | sparse, depth-1 clone of torvalds/linux master `6b4b543fa187` (7.4 merge window, 2026-10-25). Only the needed directories are checked out. Topic branches `fp6/*`. |
| `milos/` | clone of Catcrafts milos-linux `combined-stable` `63289f441152` (the tree the FP6 kernel is built from). Topic branches `fp6/*`. Remote `origin` = https://forgejo.catcrafts.net/Catcrafts/milos-linux.git (read only, no credentials). |
| `series/<topic>/` | `git format-patch --cover-letter -v1` output plus `to-cc.txt` |
| `covers/<topic>.txt` | cover-letter blurbs used by `make-series.sh` |
| `make-series.sh` | regenerates all series (and the oFono series) from the branches |

## Series

| series | tree / base | branch | patches | checkpatch --strict | To / Cc |
|---|---|---|---|---|---|
| `ufs-qcom-milos` (**RFC**) | mainline 6b4b543fa187 | `fp6/ufs-qcom-milos-spm-lvl5` | 1 | clean except: missing SoB (by design), mainline checkpatch does not know `Assisted-by:` yet, one >75-char line that is a quoted kernel log | To: Manivannan Sadhasivam, James E.J. Bottomley, Martin K. Petersen; Cc: linux-arm-msm, linux-scsi, linux-kernel |
| `qcom-battmgr-charge-uah` | mainline 6b4b543fa187 | `fp6/qcom-battmgr-charge-uah` | 1 | same; one CHECK for the existing CamelCase enum `QCOM_BATTMGR_UNIT_mAh` | To: Sebastian Reichel; Cc: linux-arm-msm, linux-pm, linux-kernel |
| `stk3310-no-irq` | mainline 6b4b543fa187 | `fp6/stk3310-no-irq` | 1 | clean except SoB / Assisted-by as above | To: Jonathan Cameron; Cc: David Lechner, Nuno Sá, Andy Shevchenko, linux-iio, linux-kernel |
| `milos-camss-fixes` | milos-linux 63289f441152 | `fp6/camss-milos-fixes` | 3 | clean except missing SoB | To: Jorijn van der Graaf (milos-linux). Milos CAMSS is not in mainline; get_maintainer lists Bryan O'Donoghue, Vladimir Zapolskiy, Loic Poulain, Mauro Carvalho Chehab, linux-media, linux-arm-msm: Cc them only once Milos CAMSS is posted upstream. |
| `milos-eswin-gesture-wake` | milos-linux 63289f441152 | `fp6/eswin-gesture-wake` | 1 | 76 errors / 145 warnings: all indentation, because the vendor driver uses 4-space indentation and the patch keeps the file's style | To: Jorijn van der Graaf (milos-linux). Driver is not in mainline (get_maintainer: Dmitry Torokhov, linux-input — only relevant once the driver is upstreamed). |

For the milos-linux series, a pull request on the Catcrafts forgejo
(fork `milos-linux`, push the `fp6/*` branch) may be preferred over mail;
ask the tree owner. The address in To: is the one used for milos patches on
LKML.

Test status, also stated in each commit message:
* **ufs-qcom-milos**: built (kernel6 = `7.2.0-fp6-nixos6`); only the sysfs
  `spm_lvl=5` read-back was checked. **Do not send as non-RFC until the
  supervised suspend test passes** (results/feature-push-20261024/suspend/README.md
  in the private workspace).
* **qcom-battmgr**: live on the FP6 (kernel5): `charge_full` 4517000 uAh.
  The patch changes behaviour for all SM8350/SM8550-variant devices; only
  the FP6 was tested.
* **stk3310-no-irq**: live on the FP6 (kernel5), proximity and ALS polling
  work. That kernel also carried the local bus-retry patch (see below).
* **milos-camss-fixes**: live on the FP6: all three cameras capture.
* **eswin gesture**: live on the FP6: double-tap wake with panel blanked.
  System suspend path untested (suspend is disabled on the FP6).

None of the mainline patches was compile-tested against mainline itself
(sparse checkout, no full build). They were compiled and run as part of the
milos 7.2 kernel and apply to mainline with offsets only. A
`make drivers/<path>.o` build on a full mainline tree before sending is
recommended.

## Not prepared for submission

| patch (kernel-build/…) | reason |
|---|---|
| `feature-push/sensors/0002-…-retry-transient-bus-errors` | Works around NAKs on the FP6's bit-banged i2c-gpio bus; IIO maintainers usually want such issues fixed in the bus/board, not with retries in a driver. Kept as local carry (`milos` branch `fp6/stk3310-bus-retry-local`). Does not apply to mainline without 0001. |
| `nixos-fp6/fp6-gap-proposals/stk36c61-binding.patch` | Not ours (Jorijn van der Graaf's v3 binding patch); STK36C61 support is already in mainline. |
| `nixos-fp6/fp6-gap-proposals/stk3310-noirq*.patch` | Earlier drafts of `stk3310-no-irq`, superseded. |
| `gmu-timeout/gmu-fw-init-timeout-100ms.patch` | Backport of Rob Clark's posted patch; not ours. |
| `qcomtee-cma/qcomtee-large-shm-cma.patch` | Device-local workaround (CMA for large QTEE shm); needs design discussion with the qcomtee maintainers before it is upstreamable. |
| `feature-push/misc/0002-EXPERIMENTAL-…-rtc-allow-set-time` | Experimental, unverified whether the PMIC accepts HLOS writes; userspace offset service is used instead. |
| `feature-push/network/0001-ath11k-wcn6750-experimental-p2p-modes` | Experimental, untested; firmware support for P2P on WCN6750 unknown. |
| `feature-push/camera/0001-fp6-s5kkd1-dt-front-rotation-270` | Patches our own out-of-tree DT include (not kernel source); rotation value not yet confirmed. |

## Sending (once configured)

    git config --global sendemail.smtpserver smtp.gmail.com     # example
    git config --global sendemail.smtpserverport 587
    git config --global sendemail.smtpencryption tls
    git config --global sendemail.smtpuser you@example.com
    # Gmail needs an app password; git send-email prompts for it (do not store it in a repo)

    SIGNOFF=1 ./make-series.sh
    git send-email --dry-run series/stk3310-no-irq/*.patch   # check headers first
    git send-email series/stk3310-no-irq/*.patch

To/Cc are already in each patch's headers (from `--to/--cc`), and listed in
`to-cc.txt`. Re-run `scripts/get_maintainer.pl` on a fresh tree before
sending, since MAINTAINERS changes. Single-patch series can be sent without
the cover letter (move its notes below the `---` of the patch).

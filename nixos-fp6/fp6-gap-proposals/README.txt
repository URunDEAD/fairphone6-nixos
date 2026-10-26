UNAPPLIED proposals, not imported by any default/trial entry.

Verified kernel645 remains pinned63289f+config.final unchanged.

front-s5kkd1-wiring.json records official Android DT and pinned-board comments.
It is not an executable sensor node: driver/mode tables, binding, actual CSI
lane/link-frequency configuration and front actuator support are missing.

stk36c61-binding.patch adapts the public reviewed v3 2/5 documentation patch
to the pinned tree. It only lets schemas recognize the already-present exact
STK36C61+STK3310 fallback compatible. It cannot fix probe or proximity.
Synthetic positive/negative binding checks are recorded under results/.

stk3310-noirq.patch is the earlier bounded polling-only init proposal:
only enablePS interrupts when client->irq>0, mirroring existing probe IRQ setup.
Current DT has no IRQ; current init nevertheless writes INTCTRL and returns
-EIO. This patch is not applied to kernel645 or physically tested. A separate
external module now builds natively against exact645 headers/release:
stk3310-polling-build.nix; metadata checks pass for AArch64/7.2.0-fp6-nixos1.
It is unactivated, not installed/copied to phone, and not imported by any
default target. See results/stk3310-polling/result.json.
Skipping the failed write is not proof of complete STK36C61 operation; real
register/bus/power behavior and ALS/proximity polling must be validated later.
No IRQ pin, voltage, sensor chip ID or register layout is guessed here.

Source pin/audit: results/fp6-public-gap-source-pins.json and
results/fp6-public-gap-source-audit.txt. No biometric, radio or camera activation.

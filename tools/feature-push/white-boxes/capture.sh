#!/usr/bin/env bash
# READ-ONLY capture for the "text turns into white boxes" bug (FP6, Lomiri/Mir,
# freedreno A810). Run it ON THE PHONE as the session user, while the boxes are
# on screen (or as soon after as possible):
#
#   bash capture.sh [--where shell|apps|both|unknown] [--note "free text"]
#                   [--since "10 min ago"] [--shot /path/to/drm-screenshot.py]
#                   [--gpu-state]
#
# --gpu-state also reads /sys/kernel/debug/dri/*/gpu (msm GPU state snapshot:
# registers, ringbuffers, IFPC/preemption state). Reading it is read-only for
# the GPU but runs the CP crash dumper and briefly holds the GPU lock, so it
# is off by default; use it only once the boxes are already on screen.
#
# From the laptop (USB or Wi-Fi ssh):
#   tools/phone-ssh 'bash -s -- --where shell' < tools/feature-push/white-boxes/capture.sh
#
# Writes only into its own output directory (default
# ~/fp6-white-boxes/<timestamp>/). It changes no setting, service, sysctl,
# debugfs knob, GPU state or process state (no signals, no SIGCONT to frozen
# apps). Root-only reads go through `sudo -n` and are skipped without a
# password-less sudo.
set -u

WHERE=unknown
NOTE=""
SINCE="15 min ago"
SHOT=""
GPUSTATE=0
while [ $# -gt 0 ]; do
    case "$1" in
        --where) WHERE=${2:-unknown}; shift 2 ;;
        --note) NOTE=${2:-}; shift 2 ;;
        --since) SINCE=${2:-15 min ago}; shift 2 ;;
        --shot) SHOT=${2:-}; shift 2 ;;
        --gpu-state) GPUSTATE=1; shift ;;
        -h|--help) sed -n '2,26p' "$0"; exit 0 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done

ts=$(date +%Y%m%dT%H%M%S)
OUT=${FP6_WB_OUT:-$HOME/fp6-white-boxes}/$ts
mkdir -p "$OUT" || exit 1
SUDO="sudo -n"
$SUDO true 2>/dev/null || SUDO=""

sec() { printf '\n## %s\n' "$*"; }
rootcat() {   # rootcat FILE [max-bytes]
    if [ -r "$1" ]; then head -c "${2:-262144}" "$1" 2>&1
    elif [ -n "$SUDO" ]; then $SUDO head -c "${2:-262144}" "$1" 2>&1
    else echo "(unreadable without sudo: $1)"; fi
}
jctl() { if [ -n "$SUDO" ]; then $SUDO journalctl "$@"; else journalctl "$@"; fi; }

# ---------------------------------------------------------------- summary
{
    echo "captured=$(date --iso-8601=seconds) where=$WHERE"
    echo "note=$NOTE"
    echo "kernel=$(uname -r) boot_id=$(cat /proc/sys/kernel/random/boot_id)"
    echo "system=$(readlink -f /run/current-system)"
    echo "uptime=$(cut -d' ' -f1 /proc/uptime)s sudo=${SUDO:-none}"
} > "$OUT/summary.txt"

# ---------------------------------------------------------------- screenshot
# Do this first, so the frame matches the report.
if [ -z "$SHOT" ]; then
    for c in "$(dirname "$0")/../../drm-screenshot.py" "$HOME/drm-screenshot.py" \
             "$HOME/tools/drm-screenshot.py" /tmp/drm-screenshot.py; do
        [ -f "$c" ] && { SHOT=$c; break; }
    done
fi
if [ -n "$SHOT" ] && [ -f "$SHOT" ] && [ -n "$SUDO" ]; then
    # drm-screenshot.py: drmModeGetFB2 + PRIME export + read-only mmap of the
    # scanout FB, PNG on stdout. Needs root (DRM master-less GetFB2 handles).
    if $SUDO python3 "$SHOT" > "$OUT/screen.png" 2> "$OUT/screen.err"; then
        echo "screenshot=screen.png" >> "$OUT/summary.txt"
    else
        echo "screenshot=failed (see screen.err)" >> "$OUT/summary.txt"
    fi
else
    echo "screenshot=skipped (no drm-screenshot.py via --shot, or no sudo -n)" >> "$OUT/summary.txt"
fi

# ---------------------------------------------------------------- journal
# Full window (user + system + kernel), then a filtered view.
jctl --no-pager -o short-precise --since "$SINCE" > "$OUT/journal.txt" 2>&1
jctl --no-pager -o short-precise -k -b 2>&1 | tail -n 20000 > "$OUT/kernel-boot.txt"
PAT='GL_OUT_OF_MEMORY|GL_INVALID|Mesa: |User error|freedreno|fd_bo|submit failed|ENOMEM|Cannot allocate|msm|adreno|a8xx|gpu fault|iommu|smmu|page fault|hangcheck|recover|lockup|devcoredump|kgsl|drm|dma.buf|dmabuf|glyph|QSG|QOpenGL|QQuick|qtmir|[Mm]ir:|EGL|ColorFilter|display-comfort|earlyoom|oom|Out of memory|killed process|app-ready|SIGSTOP|suspended|resumed|waydroid|lxc|zram|swap'
grep -E "$PAT" "$OUT/journal.txt" > "$OUT/journal-filtered.txt" 2>/dev/null
grep -E 'msm|adreno|drm|iommu|smmu|fault|hang|recover|oom|Out of memory|zram' \
    "$OUT/kernel-boot.txt" | tail -n 400 > "$OUT/kernel-gpu-mem.txt" 2>/dev/null

# ---------------------------------------------------------------- GPU / DRM
# Per-process reads (/proc/<pid>/fdinfo, environ, maps) of other users' processes
# need root, so this part runs as one root shell when sudo -n works.
cat > "$OUT/.procscan.sh" <<'PROCSCAN'
sec() { printf '\n## %s\n' "$*"; }
sec "drm clients (fdinfo), one line per DRM fd"
for fdinfo in /proc/[0-9]*/fdinfo/*; do
    grep -q '^drm-driver:' "$fdinfo" 2>/dev/null || continue
    pid=${fdinfo#/proc/}; pid=${pid%%/*}
    comm=$(cat "/proc/$pid/comm" 2>/dev/null)
    res=$(awk '/^drm-resident-/{sub(/^drm-resident-/,""); printf "%s%s%s ", $1, $2, $3}' "$fdinfo")
    tot=$(awk '/^drm-total-/{sub(/^drm-total-/,""); printf "%s%s%s ", $1, $2, $3}' "$fdinfo")
    eng=$(awk '/^drm-engine-gpu:/{print $2}' "$fdinfo")
    echo "$pid $comm fd=${fdinfo##*/} resident=${res:-?} total=${tot:-?} gpu_ns=${eng:-?}"
done 2>/dev/null
sec "drm fdinfo raw (first DRM fd per pid)"
for p in /proc/[0-9]*; do
    for f in "$p"/fdinfo/*; do
        if grep -q '^drm-driver:' "$f" 2>/dev/null; then
            echo "--- ${p#/proc/} $(cat "$p/comm" 2>/dev/null)"; cat "$f"; break
        fi
    done
done 2>/dev/null
sec "GL clients: Qt version, state and the environment that matters"
for p in /proc/[0-9]*; do
    pid=${p#/proc/}
    grep -qE 'libGLESv2|libEGL|dri_gbm|libgallium|msm_dri' "$p/maps" 2>/dev/null || continue
    comm=$(cat "$p/comm" 2>/dev/null)
    qt=$(grep -oE 'libQt[56](Quick|Gui)\.so[.0-9]*' "$p/maps" 2>/dev/null | sort -u | tr '\n' ' ')
    env=$(tr '\0' '\n' < "$p/environ" 2>/dev/null | grep -E '^(QML_|QSG_|QT_QUICK|QT_ENABLE_GLYPH|QT_QPA_PLATFORM=|QT_WAYLAND_FORCE_DPI|GRID_UNIT_PX|UITK_FONT_SCALE|MESA_|FD_MESA|GALLIUM|LIBGL|mesa_glthread|FP6_COLORFILTER|WAYLAND_DISPLAY)' | tr '\n' ' ')
    echo "$pid $comm [state $(awk '{print $3}' "$p/stat" 2>/dev/null)] qt=${qt:-none}"
    echo "    env: ${env:-(none of the watched variables)}"
done
PROCSCAN
{
    if [ -n "$SUDO" ]; then $SUDO bash "$OUT/.procscan.sh"; else bash "$OUT/.procscan.sh"; fi
    sec "devcoredump (GPU crash dumps present?)"
    ls -la /sys/class/devcoredump/ 2>&1
    sec "msm module parameters"
    for f in /sys/module/msm/parameters/*; do
        [ -r "$f" ] && echo "$(basename "$f")=$(cat "$f" 2>/dev/null)"
    done
    sec "gpu devfreq / runtime pm"
    for d in /sys/class/devfreq/*; do
        case "$(readlink -f "$d")" in *gpu*) ;; *) continue ;; esac
        echo "$d cur=$(cat "$d/cur_freq" 2>/dev/null) gov=$(cat "$d/governor" 2>/dev/null)"
    done
    for d in /sys/bus/platform/devices/*.gpu/power; do
        [ -d "$d" ] && echo "$d runtime_status=$(cat "$d/runtime_status" 2>/dev/null)"
    done
    sec "kgsl (expected absent on this msm DRM build)"
    ls -d /dev/kgsl* /sys/class/kgsl 2>&1
} > "$OUT/gpu.txt" 2>&1
rm -f "$OUT/.procscan.sh"

# devcoredump data is read-only to read (only a write discards it); size-capped.
for d in /sys/class/devcoredump/devcd*; do
    [ -e "$d/data" ] || continue
    rootcat "$d/data" 8388608 > "$OUT/$(basename "$d")-data.txt"
done
# msm debugfs (read-only files only; never "shrink", "reset" or the
# hangcheck/devfreq knobs). "gem" walks every GEM object under obj_lock and
# ends with Total/Active/Resident/Purgeable/Purged counts; it can take a
# moment with many BOs.
if [ -n "$SUDO" ]; then
    for d in $($SUDO sh -c 'ls -d /sys/kernel/debug/dri/* 2>/dev/null'); do
        n=$(basename "$d")
        $SUDO test -e "$d/gem" || continue   # GPU-capable minor only
        rootcat "$d/gem" 8388608 > "$OUT/debugfs-dri$n-gem.txt"
        {
            sec "dri/$n name";    rootcat "$d/name"
            sec "dri/$n clients"; rootcat "$d/clients"
            sec "dri/$n hangcheck_period_ms"; rootcat "$d/hangcheck_period_ms"
            sec "dri/$n gem summary (tail of gem)"
            tail -n 12 "$OUT/debugfs-dri$n-gem.txt"
            sec "dri/$n gem: purged / purgeable / evicted objects"
            grep -cE ' purged' "$OUT/debugfs-dri$n-gem.txt" | sed 's/^/purged_lines=/'
            grep -cE ' purgeable' "$OUT/debugfs-dri$n-gem.txt" | sed 's/^/purgeable_lines=/'
        } > "$OUT/debugfs-dri$n.txt" 2>&1
        if [ "$GPUSTATE" = 1 ] && $SUDO test -e "$d/gpu"; then
            rootcat "$d/gpu" 33554432 > "$OUT/debugfs-dri$n-gpu-state.txt"
        fi
    done
fi
{
    sec "transparent hugepages (msm-next enables THP for GEM; here: system setting)"
    cat /sys/kernel/mm/transparent_hugepage/enabled /sys/kernel/mm/transparent_hugepage/shmem_enabled 2>&1
    grep -E '^thp_' /proc/vmstat | head -20
    sec "msm tracepoints available (for an armed trace run; not enabled here)"
    if [ -n "$SUDO" ]; then $SUDO sh -c 'ls /sys/kernel/tracing/events/msm 2>/dev/null || ls /sys/kernel/debug/tracing/events/msm 2>/dev/null' | tr '\n' ' '; echo; fi
    sec "kernel lines: preemption / IFPC / GMU / UBWC / smmu"
    grep -iE 'preempt|ifpc|gmu|ubwc|smmu|arm-smmu|context fault|msm_dpu.*underrun' "$OUT/kernel-boot.txt" | tail -n 80
} >> "$OUT/gpu.txt" 2>&1

# ---------------------------------------------------------------- memory
{
    sec meminfo; cat /proc/meminfo
    sec "PSI"; for p in cpu memory io; do echo "$p:"; cat "/proc/pressure/$p" 2>&1; done
    sec "swap / zram"; cat /proc/swaps
    for z in /sys/block/zram*; do
        [ -d "$z" ] || continue
        echo "$z disksize=$(cat "$z/disksize") comp=$(cat "$z/comp_algorithm" 2>/dev/null)"
        echo "  mm_stat=$(cat "$z/mm_stat" 2>/dev/null)"
        echo "  io_stat=$(cat "$z/io_stat" 2>/dev/null)"
    done
    sec "vm sysctls"
    for k in swappiness page-cluster watermark_scale_factor min_free_kbytes overcommit_memory; do
        echo "$k=$(cat /proc/sys/vm/$k 2>/dev/null)"
    done
    sec "vmstat (oom/compaction/alloc stalls)"
    grep -E '^(oom_kill|allocstall|compact_stall|compact_fail|pswpin|pswpout|workingset_refault_anon|pgmajfault)' /proc/vmstat
    sec "earlyoom"
    systemctl is-active earlyoom 2>&1
    sec "top RSS"
    ps -eo pid,stat,rss,vsz,oom_score_adj,comm --sort=-rss | head -25
} > "$OUT/memory.txt" 2>&1

# ---------------------------------------------------------------- processes
{
    sec "stopped / frozen processes (state T = SIGSTOP by fp6-app-ready, t = traced)"
    ps -eo pid,ppid,stat,etime,comm,args --sort=pid | awk 'NR==1 || $3 ~ /^[Tt]/'
    sec "cgroup freezer (frozen=1)"
    find /sys/fs/cgroup -name cgroup.freeze 2>/dev/null | while read -r f; do
        [ "$(cat "$f" 2>/dev/null)" = 1 ] && echo "${f%/cgroup.freeze}"
    done
    echo "(per-process GL/Qt environment: see gpu.txt)"
    sec "systemd --user manager environment"
    systemctl --user show-environment 2>&1 | grep -E '^(QML_|QSG_|QT_|MESA_|FD_|GRID_UNIT_PX|UITK_)' || echo "(none)"
    sec "user units (failed / lomiri)"
    systemctl --user --no-pager list-units --state=failed 2>&1 | head -20
    systemctl --user --no-pager status lomiri.service 2>&1 | head -15
} > "$OUT/processes.txt" 2>&1

# ---------------------------------------------------------------- display comfort / shell state
{
    sec "display-comfort (ColorFilter: night light = blend pass, greyscale = fetch/copy pass)"
    if command -v gsettings >/dev/null; then
        for k in night-light-enabled night-light-current greyscale-active font-scale interface-scale \
                 session-font-scale session-interface-scale dark-theme-active status; do
            echo "$k=$(gsettings get com.fairphone6.display-comfort $k 2>&1)"
        done
    fi
    grep -h 'FP6 ColorFilter' "$OUT/journal.txt" | tail -5
    jctl --no-pager -b -o short-precise 2>/dev/null | grep 'FP6 ColorFilter' | tail -5
    sec "waydroid"
    systemctl is-active waydroid-container 2>&1
    command -v lxc-info >/dev/null && $SUDO lxc-info -P /var/lib/waydroid/lxc -n waydroid 2>&1 | head -5
    sec "outputs"
    for c in /sys/class/drm/card*-*; do
        [ -r "$c/status" ] && echo "$(basename "$c") $(cat "$c/status") dpms=$(cat "$c/dpms" 2>/dev/null)"
    done
} > "$OUT/shell-state.txt" 2>&1

# ---------------------------------------------------------------- versions
{
    for pkg in mesa qtbase qtdeclarative qtmir mir lomiri lomiri-ui-toolkit qtwayland; do
        printf '%s: ' "$pkg"
        nix-store -qR /run/current-system 2>/dev/null | grep -E "/[a-z0-9]{32}-$pkg-[0-9]" | head -3 | tr '\n' ' '
        echo
    done
} > "$OUT/versions.txt" 2>&1

# ---------------------------------------------------------------- quick verdict hints
{
    echo
    echo "## hints"
    c=$(grep -cE 'GL_OUT_OF_MEMORY|User error' "$OUT/journal.txt")
    echo "mesa_gl_errors_in_window=$c   (needs MESA_DEBUG=1 in that process; >0 near the onset => H1)"
    c=$(grep -cE 'submit failed' "$OUT/journal.txt")
    echo "freedreno_submit_failed=$c"
    c=$(grep -iE 'msm|adreno|a8xx|gpu|smmu|iommu' "$OUT/kernel-gpu-mem.txt" | grep -ciE 'fault|hangcheck|recover|lockup|hang')
    echo "kernel_gpu_fault_or_recovery_lines_this_boot=$c   (>0 near onset => H3)"
    c=$(ls /sys/class/devcoredump 2>/dev/null | wc -l)
    echo "devcoredumps=$c"
    echo "psi_memory=$(head -1 /proc/pressure/memory 2>/dev/null)"
    echo "oom_kills_this_boot=$(awk '/^oom_kill /{print $2}' /proc/vmstat)"
    echo "stopped_processes=$(ps -eo stat= | grep -c '^[Tt]')"
    echo "msm_enable_preemption=$(cat /sys/module/msm/parameters/enable_preemption 2>/dev/null) msm_enable_eviction=$(cat /sys/module/msm/parameters/enable_eviction 2>/dev/null)"
    for f in "$OUT"/debugfs-dri*-gem.txt; do [ -s "$f" ] && echo "$(basename "$f"): $(tail -n 6 "$f" | tr -s ' \n' ' ')"; done
} >> "$OUT/summary.txt"

cat "$OUT/summary.txt"
echo
echo "output: $OUT"
ls -la "$OUT"

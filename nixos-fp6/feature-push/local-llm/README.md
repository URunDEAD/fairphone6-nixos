# FP6 local LLM backend

`hardware.fairphone6.localLlm.enable = true` installs a Rust user service,
Qt 5/Lomiri UITK 1.3 model settings page and llama.cpp. Nothing downloads or
loads at login. Opening Settings activates the lightweight manager over the
session bus. The first chat request loads the chosen model. No phone testing has
been performed by this worker.

The assistant can embed `ModelsView` from `import Fairphone.LocalLlm 1.0` in its
settings, use the installed System Settings page, or launch
`fp6-llm-settings`. The QML module and its D-Bus bridge are Rust; the UI is QML.
The standalone desktop entry has `X-Lomiri-Touch=true`.

## Assistant API

Session bus name and interface: `com.fairphone6.LocalLlm1`.
Object: `/com/fairphone6/LocalLlm1`.

| Method | Input | Output |
| --- | --- | --- |
| Status | — | `s`: JSON status (active, loaded, pid, endpoint, idle_seconds, use_gpu, gpu_available, battery_saver, battery_saver_override, available_mib, storage_bytes, requests, reason, n_ctx, context_tokens, context_requested) |
| ListModels | — | `s`: JSON catalogue including installed, active, partial_bytes, progress, ram_mib_ctx |
| Download | `s` catalogue ID | returns immediately; failure raises D-Bus error |
| Delete | `s` ID | removes completed and partial files; unloads/clears active model if necessary |
| SetActive | `s` ID | checks file SHA-256, persists selection, unloads previous model |
| Endpoint | — | `(ss)` OpenAI base URL including `/v1`, model ID; fails if none chosen |
| Unload | — | kills/reaps llama-server, leaves selection intact |
| Configure | `tbb` idle seconds, GPU, manual battery saver | persists settings; timeout 10–3600 s |
| SetContextSize | `u` tokens | 0 = automatic (8192), else 2048–32768; persists and unloads so the next request uses it |

Signal `DownloadProgress(s id, t received, t total, s state, s message)` uses
queued/downloading/verifying/complete/failed states. `ListModels` retains the
latest progress, so a client can reconnect without missing completion. Only one
download runs at once; duplicate requests fail. Interrupted `.part` files resume
with HTTP Range. A server returning 200 instead of 206 causes a full restart.
Size and SHA-256 must match before an atomic rename to `.gguf`. On hash failure,
delete the partial file through Settings, then retry. Downloads do not implicitly
select a model. Licence links are shown before Download; clicking Download is the
user's explicit network action. No background catalogue refresh or model fetch.

Call `Endpoint` again after a service restart or active model change. Size
requests by `Status.n_ctx`: the running server's context, or (while unloaded)
what a load would pick now for the active model and free RAM. The FP6
Assistant reads it before every model call. Send
`POST <url>/chat/completions` with its returned `model`. `GET <url>/models` is
available without loading weights. Text chat, OpenAI `tools`, tool result messages
and SSE streaming pass through to llama.cpp with `--jinja`; the manager does not
execute tools. Use a client timeout of at least 130 s to allow a cold load.
Chat requests are limited to 2 MiB. Multimodal content parts are rejected; text parts and tool messages are supported.
The proxy forwards only chat and model-list
routes. Error responses use the OpenAI `error` shape. Resource refusal is HTTP
503; a stale model ID is 404. The stable proxy stays alive while weights are
unloaded. Both proxy and child bind only `127.0.0.1`, with separate ephemeral
ports by default. The module may fix the proxy port with `localLlm.port`.

The service keeps files in `$XDG_DATA_HOME/fp6-llm`, normally
`~/.local/share/fp6-llm`; models are in `models/` and selection/preferences in
`settings.json`. `--data-dir` overrides the fp6-llm directory itself. The service
runs as the phone login user (default `user`), using the existing session bus.
There is no system-wide model daemon and no login-time weight allocation.

## Memory, idle and battery policy

Context defaults to 8192 tokens ("Automatic"; Settings offers 2048–16384, the
D-Bus API up to 32768), one llama.cpp slot, 4 generation threads, 8 batch
threads, 256 batch/128 microbatch. RAM admission is per context size: the
catalogue's 2k-context RAM plus `kv_kib_per_token` (f16 KV cache, 2 x layers x
KV heads x head dim x 2 bytes, plus 16 KiB/token of compute-buffer margin) per
token above 2048, plus the 512 MiB reserve. If the requested size does not
fit `MemAvailable`, it is halved (8192, 4096, 2048) and the load uses the first
that fits; Status reports the result as `n_ctx`. A loaded server keeps its
context until it is unloaded. CPU is the default. Idle defaults to 120 s and starts
when the last response body finishes or is dropped. Streaming keeps the model
in use until the stream ends. The watchdog checks every 2 s; it unloads below
512 MiB `MemAvailable` or at memory PSI `full avg10 >= 5%`, even during inference.
A cold load additionally requires the estimated RAM at the chosen context plus 512 MiB.
RAM numbers are conservative estimates for this context, not measurements.
Missing MemAvailable refuses/unloads inference. Missing PSI is tolerated on
kernels without pressure accounting. This policy is deliberately conservative;
measure actual FP6 use before changing it.

The FP6 battery feature's `/run/fp6-battery/saver` flag is honored. The service
also checks current and legacy power-profiles-daemon `ActiveProfile` names and
supports a manual saver override in Configure. Battery saver unloads the model
and refuses new inference until disabled. Changing GPU or active model unloads
immediately and may interrupt a current response. Idle, pressure and Unload kill
and reap the child to return its RAM; systemd kills the whole service group on
manager shutdown. Readiness is checked with `/health`; exit and startup timeout
are reported instead of returning an endpoint to an unready child. A process crash
is detected on the next status/request and the next request can restart it.

## Model choices and licences

`data/models.json` is the authoritative, revision-pinned catalogue. Metadata was
retrieved from Hugging Face model APIs on 2026-10-28; SHA-256 is the LFS content
hash, not a Git blob ID. No weight downloads were needed for development.

| Model | Quant | Download (GiB) | Est. RAM (GiB), 2k | Est. RAM (GiB), 8k | Licence |
| --- | --- | ---: | ---: | ---: | --- |
| Qwen3 1.7B | Q4_K_M | 1.19 | 2.15 | 2.90 | Apache-2.0 |
| Qwen3 4B | Q4_K_M | 2.33 | 3.71 | 4.65 | Apache-2.0 |
| Llama 3.2 3B Instruct | Q4_K_M | 1.88 | 3.03 | 3.78 | Llama 3.2 Community Licence + AUP |
| Llama 3.2 3B Instruct | Q4_0 | 1.79 | 2.93 | 3.68 | Llama 3.2 Community Licence + AUP |
| Phi-4 mini Instruct | Q4_K_M | 2.32 | 3.91 | 4.75 | MIT |

With the 512 MiB reserve, Qwen3 4B and Phi-4 mini need about 5.2 GiB
`MemAvailable` for 8k; with Lomiri and apps running they may start at 4096.

Qwen3 is the recommended starting point. Use its embedded Jinja tool template;
consider `chat_template_kwargs: {enable_thinking: false}` per request to save
context and latency. Llama 3.2 has native llama.cpp tool parsing. Phi-4 mini is
trained for function calling, but its template may take llama.cpp's generic tool
fallback; test actual tool round trips before selecting it. A Jinja template
cannot by itself make an untrained model reliable at tool use.

Gemma 3 4B was evaluated as a candidate: approximately 2.32 GiB Q4_K_M and
3.8–4.5 GiB runtime, Gemma terms (not Apache/MIT). Its stock instruct template
has no native OpenAI tool schema support. It is deliberately not offered as a
recommended tool model; a dedicated tool fine-tune/template would need separate
validation and licence review. This backend supports text only and does not
install vision projector files.

## llama.cpp packaging and Vulkan assessment

The phone's pinned nixpkgs is `/nix/store/82jdygd6crh2575x13mdqjmqds5ffwfs-source`
(nixpkgs revision `0bb7ec54c848`, via the mobile-nixos npins). It already provides
llama.cpp b9842, server enabled. `nix/llama.nix` overrides that package: no
CUDA/ROCm/OpenCL/BLAS/RPC, `GGML_NATIVE=OFF`, ARMv8.6-a + NEON/FP16/dotprod/i8mm on
aarch64. Both the A720 and A520 support that ISA. Other architectures retain
nixpkgs CPU dispatch so local tests do not use ARM flags. There is no new upstream
fork or floating source. This module is intended for that pin; it requires
`pkgs.llama-cpp` and the pin's package arguments.

`enableKleidiAI = true` optionally enables the upstream-selected KleidiAI v1.24.0,
using a fixed-output Nix source and CMake FetchContent source substitution (no
build-time fetch). Default false until measured. Q4_0 is included for a meaningful
KleidiAI comparison; not all formats/ops use KleidiAI. CPU kernels remain
available for the rest. The source revision must be kept in step when updating
llama.cpp.

`enableVulkan = true` builds/installs the Vulkan variant and makes the GPU switch
available. It does not turn GPU on automatically. Vulkan uses the system Mesa
loader/ICDs, including the full phone target's patched Mesa/Turnip; it does not
bundle proprietary Adreno libraries. `--n-gpu-layers 99` requests full offload.

Vulkan on Adreno 8xx with Turnip is plausible but experimental: llama.cpp has a
portable Vulkan backend, and Turnip exposes Vulkan compute on supported Adreno
hardware. Actual success depends on this device's kernel/Mesa combination,
shader compiler, storage buffer limits and advertised 16-bit/subgroup features.
There is no measured FP6 performance claim. GPU uses unified RAM and can add
allocation pressure, heat and driver failures; Q4 speed is not necessarily better
than ARM CPU kernels. If loading fails, switch GPU off and retry; the manager
reports the failure and does not silently change preferences. Run `vulkaninfo
--summary`, a small Qwen3 chat/tool/stream smoke test, then the benchmark before
recommending it. Mesa ICD discovery and correctness need coordinator phone tests.

The child uses an explicit local model path, `--offline`, no Web UI and a cleared
llama.cpp environment. Only a user-requested Download fetches external content.
Inference uses local text inputs and loopback HTTP. The package performs no
telemetry, repository discovery, remote fallbacks or model auto-updates.

Primary references:
- [Pinned llama.cpp server](https://github.com/ggml-org/llama.cpp/blob/b9842/tools/server/README.md)
- [Function calling](https://github.com/ggml-org/llama.cpp/blob/b9842/docs/function-calling.md)
- [Build backends / KleidiAI](https://github.com/ggml-org/llama.cpp/blob/b9842/docs/build.md)
- [Mesa Turnip](https://docs.mesa3d.org/drivers/freedreno.html)
- [Qwen3 1.7B GGUF](https://huggingface.co/ggml-org/Qwen3-1.7B-GGUF)
- [Qwen3 4B GGUF](https://huggingface.co/Qwen/Qwen3-4B-GGUF)
- [Llama 3.2 3B GGUF](https://huggingface.co/bartowski/Llama-3.2-3B-Instruct-GGUF)
- [Phi-4 mini](https://huggingface.co/microsoft/Phi-4-mini-instruct)
- [Gemma 3 template](https://huggingface.co/unsloth/gemma-3-4b-it/blob/main/tokenizer_config.json)

## Build and verify

Use the phone's pinned nixpkgs with `nix-build default.nix` (or pass `pkgs` from
that pin). `package.nix` alone builds the manager/UI; `module.nix` selects runtime
packages separately. `nix-build -E 'let pkgs = import <nixpkgs> { system =
"aarch64-linux"; }; in import ./nix/llama.nix { inherit pkgs; }'` builds CPU;
add `vulkanSupport = true` or `kleidiAiSupport = true` for optional variants.

Development: `nix-shell shell.nix --run 'dbus-run-session -- cargo test --features
ui'`. The Rust fake llama-server exercises lazy loading, tools, streaming, idle
expiry across a live stream, process failures, memory/PSI, battery saver, model
selection/deletion, resumable downloads, checksum rejection and the real D-Bus
contract/signals. Tests use synthetic bytes and loopback only. `cargo test` also
works without Qt; D-Bus coverage requires a session bus. `scripts/preview` renders
the real QML plugin offscreen with a synthetic catalogue and progress, no service
or downloads. Output is `previews/models.png`.

The kernel6 full target (`../trial-lomiri-full-kernel6-fp6.nix`) imports
`./local-llm/module.nix` with `enable = true`. `build.nix` builds the
components alone (attributes `manager`, `cpu`, `vulkan`, `kleidi`) against the
pinned nixpkgs; use the `.macmini-builder/nix-remote` wrapper for aarch64.

## Coordinator phone benchmarks

Run after installing/enabling the module, closing memory-heavy apps, downloading
models explicitly and letting the phone cool. Do not run CPU/GPU benchmarks
concurrently. The script unloads inference first; avoid chatting during a run.

```
fp6-llm-benchmark-phone --output cpu.json --thermal-note 'Ambient 22 C, unplugged, case removed'
# If Vulkan was installed, pass its exact llama-bench binary:
fp6-llm-benchmark-phone --bench /nix/store/...-fp6-llama-cpp-vulkan-9842/bin/llama-bench --gpu --output vulkan.json --thermal-note 'Ambient 22 C, unplugged, case removed'
```

Optional `--model ID` (repeatable), `--repetitions`, `--prompt-tokens`,
`--generation-tokens`. Runs all matching installed models serially, verifies SHA,
then invokes llama-bench with prompt 512 / generation 128 tokens, 3 repetitions,
4 threads. Upstream JSON includes separate `n_prompt`/`n_gen` and `avg_ts`/`stddev_ts`
(tokens/s); raw records are retained. The Rust runner samples child RSS every
100 ms and thermal zones before/after, and saves per-model errors instead of
losing earlier results. RSS can miss short peaks and undercount GPU allocations;
observe system RAM separately. Note throttling, charging, case and ambient
conditions in `--thermal-note`. This is a raw kernel throughput benchmark;
assistant cold-load latency and end-to-end tool accuracy require separate tests.

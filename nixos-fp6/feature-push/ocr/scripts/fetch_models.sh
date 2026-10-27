#!/usr/bin/env bash
set -euo pipefail
mkdir -p evaluation/models
for model in text-detection text-recognition; do
  curl --fail --location --retry 2 "https://ocrs-models.s3-accelerate.amazonaws.com/${model}.onnx" -o "evaluation/models/${model}.onnx"
done
sha256sum --check evaluation/models.sha256

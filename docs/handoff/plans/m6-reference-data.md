# M6 reference data (gathered by main, 2026-10-16/17)

Source of reference installer: user's private repo vjumpkung/pixellatent-happy-meals-scripts, file
comfyui-python-3-12-happy-meals/portable_version/install_comfyui_3_12_uv_nvidia_torch_2100_portable.bat
(local read-only copy: scratchpad/ref/). That installer targets Python 3.12 / torch 2.10.0 cu130 / Windows
= this app's `win-py312-torch2100-cu130` compatibility profile. It does not verify wheel hashes itself.

## Curated nodes (FR-SETUP-9) — reference order, exact commits resolved with `git ls-remote --symref <url> HEAD` at 2026-10-16T17:11Z
| # | name | repository | default branch | commit |
|---|---|---|---|---|
| 01 | ComfyUI-MultiGPU | https://github.com/pollockjj/ComfyUI-MultiGPU.git | main | 37a73feda5804045e4f39d8c640905d62a2169d1 |
| 02 | ComfyUI-GGUF | https://github.com/molbal/ComfyUI-GGUF.git | main | f1d050fac00b0505cba519e44ad7fb18c34318b0 |
| 03 | ComfyUI-KJNodes | https://github.com/kijai/ComfyUI-KJNodes.git | main | 17195fcfa8a05f601b766a0f09c9733284f47416 |
| 04 | ComfyUI-H3-Motion-Context-MultiRef | https://github.com/seitanism/ComfyUI-H3-Motion-Context-MultiRef.git | main | 361624fb406b63eb6694442eac6c895fc1533a70 |
| 05 | ComfyUI-VideoHelperSuite | https://github.com/Kosinkadink/ComfyUI-VideoHelperSuite.git | main | 4d907bee61e92c2e65af3bd6383a4e4d356126d1 |
| 06 | rgthree-comfy | https://github.com/rgthree/rgthree-comfy.git | dev | 5f940ddfbb83ff9c667f32ae7d0c3ad0bb8cc9ee |
| 07 | ComfyUI-Crystools | https://github.com/crystian/ComfyUI-Crystools.git | main | 2f18256c5b5063937106f29a8e0a7db3ae3869b7 |
| 08 | ComfyUI-WanVideoWrapper | https://github.com/kijai/ComfyUI-WanVideoWrapper.git | main | 088128b224242e110d3906c6750e9a3a348a659b |
| 09 | ComfyUI-Custom-Scripts | https://github.com/pythongosssss/ComfyUI-Custom-Scripts.git | main | 609f3afaa74b2f88ef9ce8d939626065e3247469 |
| 10 | was-node-suite-comfyui | https://github.com/WASasquatch/was-node-suite-comfyui.git | main | 49a1098630dedff9e0ef85e8e6c4d9a4ac85f8a1 |
| 11 | ComfyUI-QwenVL | https://github.com/1038lab/ComfyUI-QwenVL.git | main | 1b67b443918801f571714bab636edc1845b7002a |
| 12 | ComfyUI-MelBandRoFormer | https://github.com/kijai/ComfyUI-MelBandRoFormer.git | main | 92c86854e6654f4aacc97484471af95c98ea16d4 |
| 13 | RES4LYF | https://github.com/ClownsharkBatwing/RES4LYF.git | main | 56e44755d866c87ff9cb4b6608c8e42155db6ecc |
| 14 | ComfyUI-SolAttn_triton | https://github.com/kijai/ComfyUI-SolAttn_triton.git | main | 26d816ebd4f1e43a2c6e4d4759be3137f10a7a73 |
| 15 | comfyui-infinitetalk-native-sampler | https://github.com/vjumpkung/comfyui-infinitetalk-native-sampler.git | main | 2bcb9e7e75348044b960d7759438c97e6b4ab289 |
| 16 | comfyui-mosaic-blur | https://github.com/vjumpkung/comfyui-mosaic-blur.git | master | ea6e4d21b81e74d1f7fbc2c18d037d23afdaf6d3 |
| 17 | ComfyUI-WanAnimatePreprocess | https://github.com/kijai/ComfyUI-WanAnimatePreprocess.git | main | 0e0b6a2a555625acf4d4aefb780e27d06937132f |
| 18 | comfyui-wan-animate-2-loop-sampler | https://github.com/vjumpkung/comfyui-wan-animate-2-loop-sampler.git | main | 64f9b89c1c478000de8ec2db464d16ef4ab2954e |
| 19 | comfyui-scail-2-loop-sampler | https://github.com/vjumpkung/comfyui-scail-2-loop-sampler.git | main | 569fa4a3c19e78a20a7dfa239955efd64fe77881 |
| 20 | ComfyUI-PlagueKind-Nodes | https://github.com/PlagueKind/ComfyUI-PlagueKind-Nodes.git | main | d58d006a4ea32c25c06499f2ff104f0852a045a6 |
| 21 | ComfyUI-mxToolkit | https://github.com/Smirnov75/ComfyUI-mxToolkit.git | main | 7f7a0e584f12078a1c589645d866ae96bad0cc35 |
| 22 | ComfyUI-MAINodes | https://github.com/matlowai/ComfyUI-MAINodes.git | main | f4868b4a08e8a504ce86db54a17961d399ffa2bc |
Reference install helper: `git clone` default branch, then runs install.py if present, then requirements.txt.

## Accelerator wheels for win-py312-torch2100-cu130 (reference installer step 9). Hashes are publisher-provided digests (GitHub release asset `digest`, Hugging Face LFS `X-Linked-ETag`, or the package index `#sha256=` fragment), observed 2026-10-16. Import verification NOT performed (no NVIDIA GPU on this machine: AMD Radeon only).
| name | module | url | filename | sha256 | size | source of hash |
|---|---|---|---|---|---|---|
| SageAttention 2.2.0 post4 | sageattention | https://github.com/woct0rdho/SageAttention/releases/download/v2.2.0-windows.post4/sageattention-2.2.0%2Bcu130torch2.9.0andhigher.post4-cp39-abi3-win_amd64.whl | sageattention-2.2.0+cu130torch2.9.0andhigher.post4-cp39-abi3-win_amd64.whl | f7261c53a1d46202059daff6b8feac5ee806274d47e3e7fb7abf8c5f6d410c92 | 12321863 | GitHub release asset digest |
| llama-cpp-python 0.3.34 cu130 | llama_cpp | https://github.com/JamePeng/llama-cpp-python/releases/download/v0.3.34-cu130-Basic-win-20260331/llama_cpp_python-0.3.34%2Bcu130.basic-cp312-cp312-win_amd64.whl | llama_cpp_python-0.3.34+cu130.basic-cp312-cp312-win_amd64.whl | f418f3fe281c1d6c9e8d996e730e955a7d8b7ad447e7d09a6a1c383e64684f40 | 311186533 | GitHub release asset digest |
| FlashAttention 2.8.3 | flash_attn | https://huggingface.co/ussoewwin/Flash-Attention-2_for_Windows/resolve/main/flash_attn-2.8.3%2Bcu130torch2.10.0cxx11abiTRUE-cp312-cp312-win_amd64.whl | flash_attn-2.8.3+cu130torch2.10.0cxx11abiTRUE-cp312-cp312-win_amd64.whl | d1e76adda81f57d9a300b9e8d71b9c467e07923e0d8e5baa5d90e2007815ec93 | 367193483 | HF X-Linked-ETag (LFS sha256) |
| FlashAttention 3 (20260318) | flash_attn_interface (verify; package dist name flash_attn_3) | https://github.com/windreamer/flash-attention3-wheels/releases/download/2026.03.19-850211f/flash_attn_3-3.0.0%2B20260318.cu130torch2100cxx11abitrue.8afc61-cp39-abi3-win_amd64.whl | flash_attn_3-3.0.0+20260318.cu130torch2100cxx11abitrue.8afc61-cp39-abi3-win_amd64.whl | 36642bbbaea59e035b3c57b3fa1dded0e40598949321917cc218c71bd79b184b | 384220928 | GitHub release asset digest (reference used --find-links https://windreamer.github.io/flash-attention3-wheels/cu130_torch2100) |
| onnxruntime-gpu 1.26.0 (CUDA 13 feed) | onnxruntime | https://aiinfra.pkgs.visualstudio.com/2692857e-05ef-43b4-ba9c-ccf1c22c437c/_packaging/22685817-9e91-4967-bafe-94c9843c26a9/pypi/download/onnxruntime-gpu/1.26/onnxruntime_gpu-1.26.0-cp312-cp312-win_amd64.whl | onnxruntime_gpu-1.26.0-cp312-cp312-win_amd64.whl | 7388afa11b005eea389e06d60c4a290494a4dd71e507a24ea21890dd9c39b547 | ? | index #sha256 fragment; reference installed with --no-deps from the ort-cuda-13-nightly feed |
Reference extra packages/constraints step: `transformers==5.5.0 onnx "numpy<2.3" "pillow<12"` (index packages, not wheels).
No verified wheels for py313/py314 + torch 2.14.1: keep those profiles' accelerator lists empty (HANDOFF instruction).

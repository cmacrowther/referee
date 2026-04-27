# Third-Party Credits and Licenses

This document lists the key libraries and tools that make this application possible, along with their respective licenses.

## Core Binaries

### FFmpeg
- **Project:** [https://ffmpeg.org/](https://ffmpeg.org/)
- **Builds:** [https://github.com/BtbN/FFmpeg-Builds](https://github.com/BtbN/FFmpeg-Builds) (BtbN)
- **License:** GNU Lesser General Public License v2.1 or later (LGPL)
- **Usage:** Input stream decoding and normalization, libplacebo/Vulkan-based scaling and HDR processing, HLS segment packaging.

### NVEncC
- **Project:** [https://github.com/rigaya/NVEnc](https://github.com/rigaya/NVEnc)
- **Author:** rigaya
- **License:** MIT
- **Usage:** Hardware-accelerated NVIDIA encoding, AI upscaling (ngx-vsr), and TrueHDR.

### VCEEncC
- **Project:** [https://github.com/rigaya/VCEEnc](https://github.com/rigaya/VCEEnc)
- **Author:** rigaya
- **License:** MIT
- **Usage:** Hardware-accelerated AMD encoding.

## Software Frameworks

### Tauri
- **Project:** [https://tauri.app/](https://tauri.app/)
- **License:** MIT / Apache-2.0
- **Usage:** Desktop application framework.

## Frontend Dependencies

- **React**: [https://react.dev/](https://react.dev/) (MIT)
- **Tailwind CSS**: [https://tailwindcss.com/](https://tailwindcss.com/) (MIT)
- **Lucide Icons**: [https://lucide.dev/](https://lucide.dev/) (ISC)

## NVIDIA Proprietary SDKs
- **Technologies:** NVIDIA Video Codec SDK (NVENC), NVIDIA NGX SDK (RTX VSR & TrueHDR), and NVIDIA Optical Flow SDK (Frame Generation).
- **Copyright:** © NVIDIA Corporation. All rights reserved.
- **License:** This software interfaces with proprietary NVIDIA technologies. Use of these features requires compatible NVIDIA RTX hardware and official NVIDIA drivers. The underlying NVIDIA SDKs and driver components are subject to their respective [NVIDIA Software License Agreements](https://docs.nvidia.com/video-technologies/). REFEREE is not affiliated with or endorsed by NVIDIA Corporation.

## AMD Proprietary SDKs
- **Technologies:** AMD Advanced Media Framework (AMF) SDK.
- **Copyright:** © Advanced Micro Devices, Inc. All rights reserved.
- **License:** This software interfaces with proprietary AMD technologies. Use of these features requires compatible AMD hardware and official AMD drivers. The underlying AMD SDK components are subject to the [AMD AMF SDK License Agreement](https://github.com/GPUOpen-LibrariesAndSDKs/AMF/blob/master/LICENSE.txt). REFEREE is not affiliated with or endorsed by Advanced Micro Devices, Inc.

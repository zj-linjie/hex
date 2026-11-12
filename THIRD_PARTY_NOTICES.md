# Third-Party Notices

Hex's own source is covered by the root `LICENSE`. The following notices cover
vendored icon assets and the speech models Hex downloads under their own
licences. Dependencies retain their respective upstream licenses.

## Parakeet Ultra

Parakeet Ultra by Moondream, based on NVIDIA Parakeet TDT 0.6B v3. GGUF
conversion and quantization by handy-computer. Creative Commons Attribution
4.0 International (CC BY 4.0). Hex downloads `parakeet-ultra-Q8_0.gguf` from
https://huggingface.co/handy-computer/parakeet-ultra-gguf/tree/a1539be1b849ea7463b88343e5277b373d0c4ea9
only while Labs › Parakeet Ultra is on; it does not ship the weights.

License: https://creativecommons.org/licenses/by/4.0/legalcode.en

Notice supplied with the files:

> Modifications: handy-computer converted
> https://huggingface.co/moondream/parakeet-ultra at commit 73175eb to GGUF for
> transcribe.cpp (https://github.com/handy-computer/transcribe.cpp). Conversion
> repackages the weights, renaming and re-laying-out tensors; the quantized
> files (Q8_0, Q6_K, Q5_K_M, Q4_K_M) also store them at reduced numeric
> precision. The weights were not retrained or fine-tuned.

The weights are provided as-is, without warranties, as the licence states.

## Orukeet

Orukeet by Oruk AI, based on NVIDIA Parakeet TDT 0.6B v3. GGUF conversion and
quantization by handy-computer. Creative Commons Attribution-ShareAlike 4.0
International (CC BY-SA 4.0). Hex downloads `orukeet-Q8_0.gguf` from
https://huggingface.co/handy-computer/orukeet-gguf/tree/c5511a3850fce1cbbfa5d94ef815966e1d99275f
and does not ship the weights.

License: https://creativecommons.org/licenses/by-sa/4.0/legalcode.en

Notice supplied with the files:

> # Attribution and license scope
>
> Orukeet is an adaptation of **NVIDIA Parakeet TDT 0.6B v3**. NVIDIA retains
> copyright in its model and upstream work. The base weights are distributed
> under [CC BY 4.0](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3).
> Oruk AI's changes comprise multilingual/accent continuation training,
> parameter blending, fitted and frozen Gabor-kernel replacement and recovery,
> native and ONNX export, application integration and evaluation.
> [Model stages](https://github.com/Oruk-AI/orukeet/blob/main/release/model-stages.json)
> identifies r3 as the source of every current Orukeet download and records its
> ancestry.
>
> The r3 NeMo, ONNX INT8, Q8 GGUF and F16 GGUF weights, and their fitted Gabor
> kernels, are designated **CC BY-SA 4.0**. The complete license is in
> LICENSE-WEIGHTS. This permits commercial use and modification, with
> attribution and applicable ShareAlike requirements.
> Earlier checkpoints retain their source notices and are not silently
> relicensed by this file. Orukeet v0.1.0 distributes the final r3 checkpoint in
> all four formats.
>
> Python and integration code in this repository is MIT unless a file specifies
> otherwise. The native bindings, audio windowing and worker transport derive
> from OpenWhisper by Knuckles92 and its Oruk AI integration; their MIT notice
> is retained in
> [LICENSE](https://huggingface.co/handy-computer/orukeet-gguf/blob/c5511a3850fce1cbbfa5d94ef815966e1d99275f/LICENSE).
> The downloaded NVIDIA NeMo-Speech.cpp SDK
> and its bundled dependencies retain their own notices, including Apache-2.0
> and MIT components. Keep the SDK's license files when redistributing it. The
> [pinned source](https://github.com/NVIDIA/NeMo-Speech.cpp/tree/4f9676226f667d14608487df744f375db87127f8)
> is the authority for those terms.
>
> The ONNX exporter follows sherpa-onnx's Parakeet TDT v3 conversion script. Its
> upstream reference and Apache-2.0 license are retained in
> [export/onnx](https://github.com/Oruk-AI/orukeet/blob/main/export/onnx/README.md).
> The ONNX archive includes the weight license and source attribution. The
> optimized ONNX encoder uses exactly equivalent FP32 arithmetic for 24
> quantized depthwise convolutions, with existing runtime operators and
> unchanged quantized values. Original export and separate optimization
> receipts are retained in the GitHub repository under
> `evidence/onnx-r3-20260910/` and `evidence/speed20260910/`.
>
> Training data credits:
>
> - Mozilla/Common Voice contributors: Common Voice 22, CC0.
> - Google and the FLEURS authors: FLEURS, CC BY 4.0.
> - Google and the OpenSLR 83 authors, with the `ylacombe/english_dialects`
>   restructuring: English dialect speech, CC BY-SA 4.0.
> - Beijing Kingline Data Technology and the SpeechOcean762 authors:
>   SpeechOcean762, CC BY 4.0.
> - SpeechColab and the GigaSpeechBench authors: 17 English accent/domain splits
>   in the final continuation. Their paper identifies Creative Commons source
>   audio; the source record below preserves the available license details.
> - DISCO at ETH Zurich and the EuroSpeech contributors: Bulgarian, Greek and
>   Italian parliamentary speech, with the providers' country-specific terms.
> - SberDevices and the Golos authors: Golos Crowd, under the
>   [Public license with attribution and conditions reserved](https://github.com/sberdevices/golos/blob/master/license/en_us.pdf).
> - Nordisk Språkteknologi, the National Library of Norway and the Alexandra
>   Institute: NST Swedish and Danish, distributed under CC0.
> - ILSP/Athena Research Center and the Lesbian Speech Corpus contributors:
>   dialect speech from Lesbos. The source card does not specify a reuse
>   license.
>
> Data were filtered, split, normalized and sampled as described in
> [data and licenses](https://github.com/Oruk-AI/orukeet/blob/main/docs/data-and-licenses.md).
> Corpus copyrights and source terms remain with their owners. Evaluation-only
> data have separate terms. No endorsement by NVIDIA, Mozilla, Google or the
> other source projects is implied.
>
> The generated statistical records in `evidence/metric-evidence.tar.gz`, the
> Gabor recovery count bundles under `training/gabor_half/results/`, and the
> FT-4035 benchmark counts under `evidence/regression-ft-20260907/` are
> designated CC BY 4.0, attributed to Oruk AI. This designation covers the
> prepared metric records; it does not relicense the original corpus
> recordings or transcripts, which are not included in that archive.
>
> The r3 numeric benchmark records in `evidence/standard-asr-r3-20260908/` and
> `evidence/domains-r3-20260908/` are released under CC BY 4.0. They contain
> error counts and recording identifiers; dataset audio remains with its
> providers.
>
> The paired ONNX application benchmark counts in
> `evidence/onnx-r3-20260910/` are also released under CC BY 4.0. These records
> contain numeric errors, timings and recording identifiers, without corpus
> audio or transcripts.
>
> Modifications: handy-computer converted https://huggingface.co/oruk/orukeet
> at commit b59c13a to GGUF for transcribe.cpp
> (https://github.com/handy-computer/transcribe.cpp). Conversion repackages the
> weights, renaming and re-laying-out tensors; the quantized files (Q8_0,
> Q6_K, Q5_K_M, Q4_K_M) also store them at reduced numeric precision. The weights
> were not retrained or fine-tuned.

The weights are provided as-is, without warranties, as the licence states.

## Wayland Input Method Protocol

`src/linux/protocols/input-method-unstable-v2.xml` is the upstream
`input-method-unstable-v2` protocol description, copied unchanged from
wayland-protocols-misc 0.3.12. It is MIT licensed; the copyright header is
kept in the file.

Upstream: https://gitlab.freedesktop.org/wayland/wayland-protocols

## Lucide And Feather

The settings sidebar icons in `assets/icons/` (`settings`, `modes`,
`meetings`, `history`, `voice-filter`, `scrawl`, `hud-lab`, `activity`) are
Lucide 1.62.0's `sliders-horizontal`, `layers`, `users`, `history`,
`audio-waveform`, `scan-line`, `flask-conical`, and `activity`, with stroke
attributes moved onto each element and a 1.75 stroke.
The `assets/icons/lucide/*.svg` assets in the Windows development branch are
also from Lucide: https://github.com/lucide-icons/lucide

Upstream license: https://github.com/lucide-icons/lucide/blob/23f9abc4ed0146cffededd3d7f94c1018bfdf693/LICENSE

`search.svg` has a modified handle endpoint and reordered SVG elements. The
complete upstream license below covers both original and derived assets.

ISC License

Copyright (c) 2026 Lucide Icons and Contributors

Permission to use, copy, modify, and/or distribute this software for any
purpose with or without fee is hereby granted, provided that the above
copyright notice and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.

---

The following Lucide icons are derived from the Feather project:

airplay, alert-circle, alert-octagon, alert-triangle, aperture, arrow-down-circle, arrow-down-left, arrow-down-right, arrow-down, arrow-left-circle, arrow-left, arrow-right-circle, arrow-right, arrow-up-circle, arrow-up-left, arrow-up-right, arrow-up, at-sign, calendar, cast, check, chevron-down, chevron-left, chevron-right, chevron-up, chevrons-down, chevrons-left, chevrons-right, chevrons-up, circle, clipboard, clock, code, columns, command, compass, corner-down-left, corner-down-right, corner-left-down, corner-left-up, corner-right-down, corner-right-up, corner-up-left, corner-up-right, crosshair, database, divide-circle, divide-square, dollar-sign, download, external-link, feather, frown, hash, headphones, help-circle, info, italic, key, layout, life-buoy, link-2, link, loader, lock, log-in, log-out, maximize, meh, minimize, minimize-2, minus-circle, minus-square, minus, monitor, moon, more-horizontal, more-vertical, move, music, navigation-2, navigation, octagon, pause-circle, percent, plus-circle, plus-square, plus, power, radio, rss, search, server, share, shopping-bag, sidebar, smartphone, smile, square, table-2, tablet, target, terminal, trash-2, trash, triangle, tv, type, upload, x-circle, x-octagon, x-square, x, zoom-in, zoom-out

The MIT License (MIT) (for the icons listed above)

Copyright (c) 2013-present Cole Bemis

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.


## Opus (assistant audio codec)

The macOS assistant statically links the Opus source bundled by opus-head-sys
0.3.3 (https://github.com/xnorpx/rust-opus). Opus copyright and conditions:

```text
Copyright 2001-2023 Xiph.Org, Skype Limited, Octasic,
                    Jean-Marc Valin, Timothy B. Terriberry,
                    CSIRO, Gregory Maxwell, Mark Borgerding,
                    Erik de Castro Lopo, Mozilla, Amazon

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions
are met:

- Redistributions of source code must retain the above copyright
notice, this list of conditions and the following disclaimer.

- Redistributions in binary form must reproduce the above copyright
notice, this list of conditions and the following disclaimer in the
documentation and/or other materials provided with the distribution.

- Neither the name of Internet Society, IETF or IETF Trust, nor the
names of specific contributors, may be used to endorse or promote
products derived from this software without specific prior written
permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT OWNER
OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF
LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

Opus is subject to the royalty-free patent licenses which are
specified at:

Xiph.Org Foundation:
https://datatracker.ietf.org/ipr/1524/

Microsoft Corporation:
https://datatracker.ietf.org/ipr/1914/

Broadcom Corporation:
https://datatracker.ietf.org/ipr/1526/
```

The Rust FFI package is offered under MIT OR Apache-2.0. Its supplied Apache
license follows:

```text
                                 Apache License
                           Version 2.0, January 2004
                        http://www.apache.org/licenses/

   TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION

   1. Definitions.

      "License" shall mean the terms and conditions for use, reproduction,
      and distribution as defined by Sections 1 through 9 of this document.

      "Licensor" shall mean the copyright owner or entity authorized by
      the copyright owner that is granting the License.

      "Legal Entity" shall mean the union of the acting entity and all
      other entities that control, are controlled by, or are under common
      control with that entity. For the purposes of this definition,
      "control" means (i) the power, direct or indirect, to cause the
      direction or management of such entity, whether by contract or
      otherwise, or (ii) ownership of fifty percent (50%) or more of the
      outstanding shares, or (iii) beneficial ownership of such entity.

      "You" (or "Your") shall mean an individual or Legal Entity
      exercising permissions granted by this License.

      "Source" form shall mean the preferred form for making modifications,
      including but not limited to software source code, documentation
      source, and configuration files.

      "Object" form shall mean any form resulting from mechanical
      transformation or translation of a Source form, including but
      not limited to compiled object code, generated documentation,
      and conversions to other media types.

      "Work" shall mean the work of authorship, whether in Source or
      Object form, made available under the License, as indicated by a
      copyright notice that is included in or attached to the work
      (an example is provided in the Appendix below).

      "Derivative Works" shall mean any work, whether in Source or Object
      form, that is based on (or derived from) the Work and for which the
      editorial revisions, annotations, elaborations, or other modifications
      represent, as a whole, an original work of authorship. For the purposes
      of this License, Derivative Works shall not include works that remain
      separable from, or merely link (or bind by name) to the interfaces of,
      the Work and Derivative Works thereof.

      "Contribution" shall mean any work of authorship, including
      the original version of the Work and any modifications or additions
      to that Work or Derivative Works thereof, that is intentionally
      submitted to Licensor for inclusion in the Work by the copyright owner
      or by an individual or Legal Entity authorized to submit on behalf of
      the copyright owner. For the purposes of this definition, "submitted"
      means any form of electronic, verbal, or written communication sent
      to the Licensor or its representatives, including but not limited to
      communication on electronic mailing lists, source code control systems,
      and issue tracking systems that are managed by, or on behalf of, the
      Licensor for the purpose of discussing and improving the Work, but
      excluding communication that is conspicuously marked or otherwise
      designated in writing by the copyright owner as "Not a Contribution."

      "Contributor" shall mean Licensor and any individual or Legal Entity
      on behalf of whom a Contribution has been received by Licensor and
      subsequently incorporated within the Work.

   2. Grant of Copyright License. Subject to the terms and conditions of
      this License, each Contributor hereby grants to You a perpetual,
      worldwide, non-exclusive, no-charge, royalty-free, irrevocable
      copyright license to reproduce, prepare Derivative Works of,
      publicly display, publicly perform, sublicense, and distribute the
      Work and such Derivative Works in Source or Object form.

   3. Grant of Patent License. Subject to the terms and conditions of
      this License, each Contributor hereby grants to You a perpetual,
      worldwide, non-exclusive, no-charge, royalty-free, irrevocable
      (except as stated in this section) patent license to make, have made,
      use, offer to sell, sell, import, and otherwise transfer the Work,
      where such license applies only to those patent claims licensable
      by such Contributor that are necessarily infringed by their
      Contribution(s) alone or by combination of their Contribution(s)
      with the Work to which such Contribution(s) was submitted. If You
      institute patent litigation against any entity (including a
      cross-claim or counterclaim in a lawsuit) alleging that the Work
      or a Contribution incorporated within the Work constitutes direct
      or contributory patent infringement, then any patent licenses
      granted to You under this License for that Work shall terminate
      as of the date such litigation is filed.

   4. Redistribution. You may reproduce and distribute copies of the
      Work or Derivative Works thereof in any medium, with or without
      modifications, and in Source or Object form, provided that You
      meet the following conditions:

      (a) You must give any other recipients of the Work or
          Derivative Works a copy of this License; and

      (b) You must cause any modified files to carry prominent notices
          stating that You changed the files; and

      (c) You must retain, in the Source form of any Derivative Works
          that You distribute, all copyright, patent, trademark, and
          attribution notices from the Source form of the Work,
          excluding those notices that do not pertain to any part of
          the Derivative Works; and

      (d) If the Work includes a "NOTICE" text file as part of its
          distribution, then any Derivative Works that You distribute must
          include a readable copy of the attribution notices contained
          within such NOTICE file, excluding those notices that do not
          pertain to any part of the Derivative Works, in at least one
          of the following places: within a NOTICE text file distributed
          as part of the Derivative Works; within the Source form or
          documentation, if provided along with the Derivative Works; or,
          within a display generated by the Derivative Works, if and
          wherever such third-party notices normally appear. The contents
          of the NOTICE file are for informational purposes only and
          do not modify the License. You may add Your own attribution
          notices within Derivative Works that You distribute, alongside
          or as an addendum to the NOTICE text from the Work, provided
          that such additional attribution notices cannot be construed
          as modifying the License.

      You may add Your own copyright statement to Your modifications and
      may provide additional or different license terms and conditions
      for use, reproduction, or distribution of Your modifications, or
      for any such Derivative Works as a whole, provided Your use,
      reproduction, and distribution of the Work otherwise complies with
      the conditions stated in this License.

   5. Submission of Contributions. Unless You explicitly state otherwise,
      any Contribution intentionally submitted for inclusion in the Work
      by You to the Licensor shall be under the terms and conditions of
      this License, without any additional terms or conditions.
      Notwithstanding the above, nothing herein shall supersede or modify
      the terms of any separate license agreement you may have executed
      with Licensor regarding such Contributions.

   6. Trademarks. This License does not grant permission to use the trade
      names, trademarks, service marks, or product names of the Licensor,
      except as required for reasonable and customary use in describing the
      origin of the Work and reproducing the content of the NOTICE file.

   7. Disclaimer of Warranty. Unless required by applicable law or
      agreed to in writing, Licensor provides the Work (and each
      Contributor provides its Contributions) on an "AS IS" BASIS,
      WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or
      implied, including, without limitation, any warranties or conditions
      of TITLE, NON-INFRINGEMENT, MERCHANTABILITY, or FITNESS FOR A
      PARTICULAR PURPOSE. You are solely responsible for determining the
      appropriateness of using or redistributing the Work and assume any
      risks associated with Your exercise of permissions under this License.

   8. Limitation of Liability. In no event and under no legal theory,
      whether in tort (including negligence), contract, or otherwise,
      unless required by applicable law (such as deliberate and grossly
      negligent acts) or agreed to in writing, shall any Contributor be
      liable to You for damages, including any direct, indirect, special,
      incidental, or consequential damages of any character arising as a
      result of this License or out of the use or inability to use the
      Work (including but not limited to damages for loss of goodwill,
      work stoppage, computer failure or malfunction, or any and all
      other commercial damages or losses), even if such Contributor
      has been advised of the possibility of such damages.

   9. Accepting Warranty or Additional Liability. While redistributing
      the Work or Derivative Works thereof, You may choose to offer,
      and charge a fee for, acceptance of support, warranty, indemnity,
      or other liability obligations and/or rights consistent with this
      License. However, in accepting such obligations, You may act only
      on Your own behalf and on Your sole responsibility, not on behalf
      of any other Contributor, and only if You agree to indemnify,
      defend, and hold each Contributor harmless for any liability
      incurred by, or claims asserted against, such Contributor by reason
      of your accepting any such warranty or additional liability.

   END OF TERMS AND CONDITIONS

   APPENDIX: How to apply the Apache License to your work.

      To apply the Apache License to your work, attach the following
      boilerplate notice, with the fields enclosed by brackets "[]"
      replaced with your own identifying information. (Don't include
      the brackets!)  The text should be enclosed in the appropriate
      comment syntax for the file format. We also recommend that a
      file or class name and description of purpose be included on the
      same "printed page" as the copyright notice for easier
      identification within third-party archives.

   Copyright [yyyy] [name of copyright owner]

   Licensed under the Apache License, Version 2.0 (the "License");
   you may not use this file except in compliance with the License.
   You may obtain a copy of the License at

       http://www.apache.org/licenses/LICENSE-2.0

   Unless required by applicable law or agreed to in writing, software
   distributed under the License is distributed on an "AS IS" BASIS,
   WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
   See the License for the specific language governing permissions and
   limitations under the License.
```

## Tungstenite (assistant WebSocket transport)

Tungstenite 0.28.0 (https://github.com/snapview/tungstenite-rs), MIT license:

```text
Copyright (c) 2017 Alexey Galakhov
Copyright (c) 2016 Jason Housley

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
```

## ECDICT

ECDICT by Lin Wei (skywind3000), MIT License. Hex downloads `ecdict.csv` and
`lemma.en.txt` from https://github.com/skywind3000/ECDICT at git
`39cfb9672c0906b63569e0453c2e6abfb9ab182a` only after the user installs the
translation dictionary; the files do not ship in the app. `ecdict.mini.csv` is
not used.

Pinned artifacts (content SHA-256 of the downloaded blobs):

- `ecdict.csv` 66009548 bytes, SHA-256 `d1ecbdf9ee2b920afb6f430ca6b6bb9f559588cf0dff86e04bd255f9390843cc` (git blob `0e527e22e0e0d22f012b0ed251a8dddd5891773b`)
- `lemma.en.txt` 2318694 bytes, SHA-256 `e255b097404e3e0052060e2ddf6e15a1414f577071d63d51d2ca0ce9dacee0fc` (git blob `34eabb9f48c5867a91c01c33b206120e275f0418`)

License: https://github.com/skywind3000/ECDICT/blob/39cfb9672c0906b63569e0453c2e6abfb9ab182a/LICENSE

```text
MIT License

Copyright (c) 2025 Linwei

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

# Hex 翻译视觉原型生成提示词

生成方式：内置 `image_gen`，使用 `imagegen` 技能；参考图为仓库已有的 `tests/golden/rune-mac-settings-dark.png`。输出为视觉原型，不是 Rune 实际渲染或已实现功能。

提示词如下（原样保存）：

Use case: ui-mockup.
Asset type: one high-fidelity visual prototype board for Hex's macOS translation feature, showing its native main page and independent shortcut window. This is a generated design proposal, not a functioning screenshot.
Input image role: existing Hex Rune settings screenshot is the visual reference and source layout to transform. Preserve its restrained charcoal OpenCode theme, deep sidebar, raised rounded content panel, thin borders, simple line icons, typography rhythm and spacing. Transform the General page into the Translation page. Do not reproduce General settings.

Composition: wide landscape, flat front-on, perfectly legible app UI, no perspective or device frame. Large main Hex window on the left occupying roughly 70% of canvas; separate compact translation window on the right occupying the remaining space, visually separate and never obscuring main content. A tiny outside caption reads "视觉原型 · 示例内容". Neutral very dark background, restrained shadows, no marketing headline or decorative graphics. Main app should resemble a practical Rune/rune-kit application, not a website.

UI framework constraints: every element must map to existing Rune/rune-kit TextInput or TextArea, Button, IconButton, Badge, Select, section cards, note and keycaps. No invented charts, gradients, glassmorphism, waveform or AI chat UI. Use theme colors from reference, near white contrast-filled main button, secondary actions subtle outlined/ghost. At most one primary button PER window. Generous but compact 24–32 px gutters. Chinese uses a clear system sans font, English and IPA sharp and readable.

Main window:
Native subtle title bar labeled "Hex".
Sidebar with simple line icons, labels "常规", "输出", "历史", "翻译", "语音助手", and "Labs" last. "翻译" selected with the same medium-gray selection style as reference. Bottom footer "Hex".
Content header "翻译", at top right a subtle secondary button "打开小窗" with diagonal arrow icon.
Small muted language line "英文 → 简体中文".
One compact multiline input card containing "hex". Below input, a secondary "粘贴" action on left and one primary near-white button "查询" on right.
Below, result card with generous padding:
Large word "hex" and next to it a small quiet "单词" badge.
Under word, show ONE phonetic transcription "/hɛks/" and muted label "音标". Do not duplicate this phonetic into two region variants.
Next line two small secondary pill/rectangular controls with speaker icons: "英式朗读" and "美式朗读"; a small dropdown "正常语速" at far right. These are local system synthesis buttons, not downloaded human recordings.
Thin horizontal divider.
Clean dictionary rows:
"n." followed by "十六进制；诅咒"
"vt." followed by "施魔法；诅咒"
No example sentences or invented extra dictionary fields.
Bottom of card muted source "本地词典" and small secondary action "复制释义".
Under result, a small resource note "朗读使用系统声音".
A collapsed settings row "翻译设置" with chevron and keycaps "⌃" "⌥" "T".
A low-emphasis footer menu/link "联网重译…" only a secondary action; it opens a manual confirmation and NEVER implies automatic upload. No provider name or API key visible.

Independent compact window on right:
Subtle macOS traffic lights and header "翻译"; secondary ghost arrow button labeled "在 Hex 中".
Language line "英文 → 简体中文".
Input "Please check the file."
Secondary "粘贴", one primary "翻译".
Below a result section titled "中文" and clear readable translation "请检查这个文件。".
Secondary buttons with simple line icons "朗读原文" and "复制译文".
Muted source note "系统本地翻译".
Bottom small keycaps "⌃" "⌥" "T".
No sidebar, no chat transcript, no history, no auto-translation toggle.

Text must match the requested Chinese labels and English content verbatim, use crisp text, avoid random filler text or misspelled characters. Distinguish the main word result from the popup short-sentence result clearly. Do not add OpenCode as a translation service, REST routes, technical backend labels, fake latency, model names, microphone controls or consent banners. Ensure all requested widgets can be composed with ordinary native Rune components.

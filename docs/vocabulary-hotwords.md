# 词汇表（热词表）维护记录

个人 fork 的听写词汇表台账。调整词表时更新本文件并提交，保证记录与
`settings.json` 的 `vocabulary` 数组一致。

- 数据源：`~/Library/Application Support/hex/settings.json`
- 最近一次整理：2026-11-07（依据 7 天听写历史 210 条 + Obsidian 近 90 天 338 篇笔记的词频分析）

## 当前状态

| 项 | 值 |
| --- | --- |
| 词条总数 | 45 |
| Labs › Vocabulary biasing | **on**（`labs.vocabulary_biasing: true`，关着时识别完全不读词表） |
| Boost | `normal`（只影响 Parakeet；不要开 strong，实测会产生误替换） |
| 数字修正 | Global 模式转换链第一位 `latin-digit-fix`（`~/.config/hex/hex.config.ts`） |

## 机制备忘（调整前先读）

- 日常中文听写走 **Qwen3-ASR**：词表按列表顺序拼成逗号串作为上下文
  prompt，**预算 1000 字符**，超了从尾部丢——所以重要的词放前面。
- **sounds_like 不进 Qwen 的 prompt**，只对 Parakeet（英文模型）的偏置生效。
- Parakeet 路径**跳过含下划线的词条**（`music_pre` 在 Parakeet 下不会被偏置）。
- Live text 预览走流式路径不带词表；以最终粘贴文本为准。
- 词表只放**私有词和型号**；模型本来就认识的常用词是浪费预算且有回声风险。

## 词表（45 条，2026-11-07）

### 口语高频（2026-11-07 新增，历史实测说过）

| 词 | sounds_like | 依据 |
| --- | --- | --- |
| Codex | — | 7 天 14 次 |
| GitHub | — | 8 次 |
| zcode | z code | 8 次 |
| Hex | — | 8 次 |
| Git | — | 7 次 |
| DeepSeek | — | 6 次 |
| GLM | G L M | 4 次 |
| ChatGPT | — | 4 次 |
| iOS | — | 6 次 |
| Workbench | — | 6 次 |
| Ranger | — | 4 次 |
| 棱角 | — | 6 次 |
| prompt | — | 修错听（promit） |
| API | — | 修错听（APIT） |
| ESP32 | — | 修数字转写（ESB三二） |

### 笔记活跃的保留词（近 90 天 Obsidian 高频， spoken 待观察）

esp-xz（esp 小智）58 次、sidebar-team 58、person-web 45、wx-cli 44、
multi-style-publisher 35、music_pre 33、breadboard-studio 16、
pandacode-private 6、t1keys 4。

### 其余保留词（低频但暂留观察）

mac-tools-gateway（mac tool gateway）、MediaCrawler、night-notes-cover、
OpenBiliClaw、OpenCLI、person-web（person web）、personal-ip（person ip）、
session-for-project、video-pipeline、wechat-Miniprogram、wechat-publisher、
xiaozhi-voice（小智 voice）、agent-hud-open、chat、chat-analyzer、
chat-macbook（chat macbook）、clone-my-voice、compass_guid、
content-speed-demo、ajiang-vibe-coding-skills（A jiang vibe coding skills）、
CH-pipline（C H pipeline）、Llenger（林杰）。

### 待定：硬件词（未加，说一声就加）

`L05C`（L zero five C）、`P1S`（P one S）、`小爱音箱`、`嘉立创`。

## 2026-11-07 删除的死词（历史 + 笔记双零命中，勿再手加）

session-docter、vibewatchos、ezviz-eye、Wechat-ZY、x-quota、zcode-status、
agent-hud-remote-probe、chat-lion、deepseek-status、open-design、open-minis、
oil-find、navicat-project、music-tesla、monstermouse-analyze、
vaultmind-plugin、publisher-assistant、Pi。

## 怎么调整

```sh
hex vocabulary list                        # 看当前词表
hex vocabulary add <词> [--sounds-like "a,b"]  # 加词（对运行中的 Hex 热生效）
hex vocabulary remove <词>                 # 删词
hex labs on|off vocabulary-biasing         # 开关偏置
hex history --since 7d --json              # 复查素材
```

改完词表同步更新本文件（`docs/vocabulary-hotwords.md`）再提交。

## 维护节奏

每季度（或发现新的高频错听时）跑一次三源对比：`hex history` 词频、
Obsidian 近 90 天笔记词频、当前词表——删双零死词、进口语高频词、
把反复错听的词补进来。上次分析脚本与结论见当次会话记录。

## 相关

- 数字形态（ESP三二→ESP32、PR六十四→PR64）：`latin-digit-fix`
  转换，见 `~/.config/hex/hex.config.ts`，不占词表预算。
- 功能文档：[vocabulary.md](features/vocabulary.md)；调研结论：
  [research/custom-vocabulary.md](research/custom-vocabulary.md)。

# VERIFICATION — vizir 技术长图证据总表

本文件是交付的自证材料：证据来源、门禁结果、事实核查、修正记录、指纹、
重建确定性与环境。所有声明可亲手复现；未做或测不过的直说（见 §6、§7、§9）。

## 1. 证据与 provenance

| 项 | 值 |
|---|---|
| 引擎仓库 | `~/projects/plot/vizir`（严格只读） |
| 引擎 commit | `52840b807ee6efcee8927ab60c7aec81ad0998df` |
| vizir 版本 | `vizir 0.1.0` · rustc `1.98.0 (88d9e12ae 2026-08-18)` |
| 冻结方法 | `prep_data.py` 于干净沙盒 `/tmp/vizir-explainer-freeze` 一次运行，全部 `data/*.json` 由真实 CLI 输出导出；交付内永不重跑 |
| 主示例 | `examples/chart/service-health.viz.yaml`（sha256 见 `data/engine.json:example_sha256`） |
| 工作树基线 | 构建前已存在：`M README.md`、`M justfile`、未跟踪 `examples/mixed/capacity-planning.viz.yaml`、`gallery*`、`tools/`（用户既有工作，非本交付产生；冻结于 `data/engine.json:dirty_status_baseline`） |
| 引擎改动 | 本交付新增 `docs/infographics/vizir-explainer/`（未跟踪），未改任何已跟踪文件、零 commit（§8 复核） |

冻结的数据面（页面每个数字的出处）：`explain_samples.json`（6 查询
verbatim）、`capability.json`（两后端能力面 + 174/111+63 decision + 1 loss +
rasterizer）、`fail_loud.json`（CAP-0001 实验 exit/stderr/文件不存在性）、
`scene_nodes.json`（110 节点 + generated_by 直方图 + Origin 样本）、
`determinism.json`（三对 sha + manifest 噪声实验）、`diag_codes.json`
（81 码 ×14 族 + 每码 **grep 遍历序**首个出现 file:line——是单次
`grep -rn` 遍历顺序下的首见，非按 (file,line) 排序的真首个；§7/R12）、
`patch.json`（5 测试行号 +
14 码 + scene-patch schema sha）、`schemas.json`（3 schema sha + 防漂移
一致）、`tests.json`（62 = core 27 + compiler 5 + backend-svg 2 + cli
0+15+13，逐套件；§7/R4）、`mir_provenance.json`、
`examples.json`（工作树 11 = 4+3+2+2，其中 mixed/capacity-planning 为
未跟踪用户 WIP；git tracked 为 10，fresh clone 复现以 10 为准；§7/R6）。

## 2. 门禁总表

| 门禁 | 口径 | 结果 |
|---|---|---|
| svg-linter | 13 个 SVG（12 面板 + 页脚）逐文件 `check --plain`；另以 `--require-complete` 复核规则完整性（空 `<text>` 死标记会让规则 partial 跳过，修复前 04/08 该口径 exit 1，见 §7/R7） | **13 × exit 0，`grep -c '^finding'` 全 0；`--require-complete` 13 × exit 0**（TSV 的 outcome 行不计入） |
| build.py 断言 | 面板数 == 12；7 个关键计数（7/62/81×14/110/174/11）以 stat-tile 锚定形态 `>N</text>` 断言（裸子串会被 174/110 结构性遮蔽，§7/R11）；`<script`、`src="http`、`@import`、`fetch(` 全 0 | 全绿（篡改自测实测：改 7→8 重建 exit 1，§7/R11） |
| 代码细节清扫 | 2026-09-03 改版新增（§11）：六式清扫（file:line / 引擎源码文件名 26 个快照 / N–M 行区间 / 第N行 / 源码关键字 / 标识符调用串）+ 25 个禁用标识符 + 10 个新形式 derived needle + 声明编号 E1–E6 覆盖，全部内建于 build.py，页内全零 | 全绿（独立脚本复扫一致为 0，§11.2） |
| 渲染断言 | shoot.js：页面宽 == 1200 CSS px；stitch.py：位图高 == 页面 CSS 高 × 2 | 2400×17116 == 1200×(8558×2) ✓（二轮修复未改任何面板高度） |
| 重建确定性 | 连续两次 `python3 build.py` 后 `cmp` | **byte 级一致** |
| 引擎只读 | `git -C ~/projects/plot/vizir status --porcelain` 与基线 diff | 终态与基线**逐行相同**（基线快照时 `docs/infographics/` 已存在，§8） |

### 2.1 门禁首轮 findings 与修复（如实记录）

首轮 `svg-linter` 13 文件中 4 文件共 **5 条 finding**，全部修复后复检 13×0：

| 文件 | 规则 | 位置 | 修复 |
|---|---|---|---|
| 03-origin | out-of-bounds | text[32] x=785 w=557（JSON 框 explanation 行超画布） | 长行按词边界拆两行 |
| 04-explain-tree | out-of-bounds | text[23] y=815（第 6 张卡超出 812 画布） | 面板高 812→896 |
| 11-patch-equivalence | text-text-overlap | text[36/37] x≈548（测试名与释义列叠字） | 释义列右移至 x=593 |
| 12-gates | text-text-overlap ×2 | x≈886（schema 名与 ✓ 列叠字） | ✓ 列右移 28px |

同轮目检另修 4 处非门禁缺陷：hero「编译**不**为」笔误→「编译**前**为」；
origin JSON 切分从 `sc|ales` 改词边界 `through |scales`；explain 卡 reason
截断阈值 96→150（版面实测可容，避免截断真实输出）；「五查询」→「六查询」。

> **勘误（2026-09-02，对抗验证确认）**：上段「origin JSON 切分改词边界」
> 当时**并未真正落盘**——修复只写进了本记录，产物仍是断词版（03 号 svg
> 指纹 `68031e0a…` 恰是 `through scal|es` 断词版，记录与产物自相矛盾）。
> 已于二轮真正修复（词边界下标 29，`rfind(' ',0,34)+1`），见 §7/R1。
> 撤回不删除：本条保留原始记录并以此勘误。

## 3. 事实核查表（机制声明 vs 引擎实况，均亲手复核）

| 面板声明 | 核查点 | 结果 |
|---|---|---|
| Origin 六字段（hir_node/mir_node/data_key/data_lineage/generated_by/explanation） | `crates/vizir-core/src/scene.rs:132-142` struct Origin | ✓ |
| explain 输出逐字（gateway 等 6 查询） | 实跑 `vizir explain …--node …`，与 `data/explain_samples.json` 一致 | ✓ |
| capability 谈判逐节点 decision | `capability.rs:134` scene_capability_requirements、`:152` negotiate_scene；manifest 实测 174 条（feature/status/reason/source） | ✓ |
| 不支持即报错 | `capability.rs:127-131` VIZ-CAP-0002 汇总 Error 决策；CLI 侧 VIZ-CAP-0001 实测 exit 1 且输出/manifest 均不存在（`main.rs:334`） | ✓ |
| png 111 rasterized + 63 exact | png manifest 实测 Counter；63 = paint.alpha 保持 exact | ✓ |
| 1 条 loss record + rsvg-convert | png manifest 实测 `losses:[{fidelity:"rasterized",…}]`、`rasterizer:"rsvg-convert"`；svg/Scene2D 侧 0 loss | ✓ |
| revision 四道门 | `patch.rs:77-82`(0002 diff 侧)、`:118-123`(0003)、`:124-129`(0004)、`:130-135`(0005)、`:136-141`(0002 apply 侧) 逐一比对 | ✓ |
| 14 码的两侧分布 | 逐码 grep 发射行：0001@73 仅 diff_scene；**0002 两侧都发射**（diff@79 / apply@138，apply 侧校验在 :136-141）；0003@120…0014@401 共 12 个在 apply 路径——apply 路径可发射 13 个码 | ✓（面板 10 原写「diff 2 + apply 12」漏算 apply 侧 0002，已改写；§7/R5） |
| patch ≡ 全量重算 | `patch.rs:457` 测试，`:497` `assert_eq!(actual, next)`（严格相等，强于面板所写 ≈）；reorder order 实值 `["b","c"]`（`:490`） | ✓ |
| 81 码 ×14 族 | `grep -rn -o 'VIZ-[A-Z]*-[0-9]{4}'` 全 crates 枚举去重 = 81；族计数见 `data/diag_codes.json` | ✓ |
| schema 防漂移 | `vizir schema <ir>` 三份输出与 `schemas/*.json` byte 相等；测试 `tests/cli.rs:312` | ✓ |
| 62 测试 | 实跑 `cargo test --workspace`（二轮按套件重取，exit 0）：vizir-core(lib) 27 + vizir-compiler(lib) 5 + vizir-backend-svg(lib) 2 + vizir-cli bin 0 + tests/cli.rs 15 + tests/pipeline.rs 13 | ✓（首轮「34 unit」为三 crate 同名套件误并，§7/R4） |
| 双跑 sha | normalize/lower/render 各两跑：`5898b8dc…`/`70b6e2a9…`/`6a2caf37…` 三对全等 | ✓ |
| nice-domain + provenance 字符串 | `lower.rs:165-166`、`:217-222`；MIR 实测 4+3 条 provenance 字符串 | ✓ |

## 4. 渲染与指纹 sha256

二轮修复（对抗验证后，§7）重建并重拍后的现行指纹：
【2026-09-03 改版勘误：下表为**改版前**（二轮后）指纹，已由 §11.4 的改版轮
指纹取代；留档不删。】

| 产物 | sha256 |
|---|---|
| index.html（自包含，零外链） | `2c109272fd780c3f00e6e2e038d84f81fbc41536b95081e27e9de1cbd79be0be` |
| render/full@2x.png（2400×17116） | `f508ed3cef18eb5d8abb4906f9a677b8345fa678a793497e7f7e62814d76d7d6` |
| render/full@2x.gray.png | `5d1284640e180c5b6348aec710b8d3891c4b0bc373225204b8eaab5081638666` |
| render/thumb.png（600×4279） | `eda53bcafa1eca040ac7a9f46eff0ef2c3adae56e2636d9b88204c2d643ee750` |
| data/tests.json | `c9efee9b26039a45803c70dc1446a31a4396466941aa636870f7f37043530fcf` |

面板 SVG 指纹（13 件）：01 `23b6bd0b…`、02 `bd2016f0…`、03 `4373d91d…`、
04 `252d5851…`、05 `c1519756…`、06 `8ac12061…`、07 `60a5d7b8…`、
08 `60c852ee…`、09 `c44dcf6c…`、10 `3775e79f…`、11 `2e48718a…`、
12 `2857e29a…`、99 `44738feb…`（完整值 `shasum -a 256 svg/*.svg` 可复算）。

### 4.1 指纹迁移表（二轮修复：旧 → 新，一行一由）

| 产物 | 旧 sha256 | 新 sha256 | 变更由 |
|---|---|---|---|
| index.html | `420e1840…` | `2c109272…` | 嵌入面板 01/02/03/04/08/10/12 变更（下列各行） |
| svg/01-hero | `6e1feda0…` | `23b6bd0b…` | R2 词边界换行（deci\|sion）+ R6 examples 口径注 |
| svg/02-pipeline | `d1b10b9d…` | `bd2016f0…` | R2 词边界换行（dashboa\|rd 整词下移） |
| svg/03-origin | `68031e0a…` | `4373d91d…` | R1 词边界切分（下标 29）+ R3 精确口径与省略规则 |
| svg/04-explain-tree | `0963b647…` | `252d5851…` | R7 空文本不再发射 + R14 连字符键名（chip/着色/extra 行） |
| svg/08-fail-loud | `c351b7f9…` | `60c852ee…` | R7 空 `<text>` 间隔行不再发射（占位行距保留） |
| svg/10-patch-gate | `0e9bfe2e…` | `3775e79f…` | R5 0002 双侧发射口径 + `:136-141` 锚点 |
| svg/12-gates | `f4113035…` | `2857e29a…` | R4 逐套件测试计数 |
| data/tests.json | `09717f08…` | `c9efee9b…` | R4 per_crate 误并 → per_suite 真分解（62 不变） |
| render/full@2x.png | `f10758fb…` | `f508ed3c…` | 页面位图随面板变更重拍（尺寸不变 2400×17116） |
| render/full@2x.gray.png | `bae8cd37…` | `5d128464…` | 同上（灰度版） |
| render/thumb.png | `bef4e58c…` | `eda53bca…` | 同上（600×4279） |

**未变**：svg/05 `c1519756…`、06 `8ac12061…`、07 `60a5d7b8…`、09 `c44dcf6c…`、
11 `2e48718a…`、99 `44738feb…` 六件与其余 13 份 `data/*.json` 逐字节不变
（R4 沙箱只重取 tests.json，见 §7/R4）。

## 5. 重建确定性

- `python3 build.py` 连续两次 → `cmp` byte 级一致（无时间戳、无随机序；
  JSON dict 遍历顺序固定于冻结文件）。
- 渲染管线确定性未做多轮重拍对比（位图指纹仅一次终拍）——页面上**没有**
  任何声明依赖「PNG 重拍逐字节一致」；PNG 明确标注为呈现层（「08 · 诚实的
  损耗」面板，文件 09-loss.svg）。

## 6. 修正记录（撤回不删除）

1. **冻结器缺陷与重冻**：首次冻结 `tests.json` 解析为空（cargo 的
   `Running` 行走 stderr，合并流后修复）、patch 测试行号算到 `#[test]`
   属性行（改为 fn 起始行）。缺陷在冻结阶段当场发现，清空 `data/` 与
   沙盒后**一次性重冻**——现存 `data/` 全部来自最终那次干净运行。
2. **svgkit `el()` 闭合标签缺 `>`**：首轮 13 个 SVG 全部解析失败
   （exit 3 invalid-svg）；修复后进入正常门禁流程（§2.1 的 5 条 finding
   即修复后首轮真实结果）。
3. **brief 与实况不符处（以实况为准）**：
   - brief 称 81 稳定诊断码属 7 族 `{CAP,EXPR,PATCH,LAYOUT,MIR,BACKEND,ARTIFACT}`——实测**总数 81 正确，但族数是 14**（另含 VALIDATE/TYPE/RESOLVE/SCENE/SCHEMA/LOWER/EXPLAIN）；页面按 14 族呈现。
   - brief 引 `scene.rs:137-141`——实测 struct Origin 跨 `132-142`（137 是 data_key 行）；页面引 132-142。
   - brief 引「cli.rs:312」——实为 `crates/vizir-cli/tests/cli.rs:312`（集成测试文件），行号属实。
   - brief 其余关键声明（174 decisions、1 loss、62 测试、双跑 sha、7 子命令、explain 输出、PNG 外部 rasterizer 噪声、manifest 唯一变量为 output 路径）**逐项复核属实**。

## 7. 二轮勘误（对抗验证后，2026-09-02）

独立对抗验证（8 检查 + 敌意复核 + 主会话仲裁）确认 13 项根因；修复闭环
逐项记录如下（撤回不删除：一轮记录全部保留，§2.1/§3 另有就地勘误注）。
所有页面级修复均重建、过全部门禁、重拍位图；指纹迁移见 §4.1。

1. **R1（P2）03 面板 explanation 断词**：`through scal|es`——§2.1 曾声称
   已改词边界但产物未变（指纹 `68031e0a…` 即断词版）。根因 panels.py
   硬编码切片 `[:33]/[33:]`。**fixed**：词边界切分
   `rfind(' ',0,34)+1 = 29`，重建后 03 号指纹 `4373d91d…`，§2.1 已附勘误注。
2. **R2（P3）两处拉丁词中断行**：hero `deci|sion`、pipeline `dashboa|rd`。
   根因 wrap_cn 纯字符贪心。**fixed**：wrap_cn 对 `[A-Za-z0-9._/-]` 连续段
   整词计宽、放不下整词下移（CJK 仍可任意断）；三个调用点（hero/pipeline/
   loss 卡片）逐串断言零断词、字符零丢失，行数不变（无版面位移）。
3. **R3（P3）03 面板「110/110 都有这六个字段」口径过宽**：实测 110/110 带
   origin 对象，但 6 键全有仅 24/110（data_key 省 86、data_lineage 省 74，
   scene.rs:136/138 serde skip；四字段必现）。**fixed**：改精确口径
   （110/110 带 origin；四字段必现；可选两键省略 86/74）+ JSON 框旁一行
   省略规则说明。data 未改（scene_nodes.json 原样保留全量节点数据）。
4. **R4（P3）tests.json 的 per_crate 误并**：`"unittests src/lib.rs": 34`
   实为 vizir-core 27 + vizir-compiler 5 + vizir-backend-svg 2 三个 crate
   同名套件合并（键取自 Running 行 target 路径，无 crate 名）。**fixed**：
   同引擎同 commit 重跑 `cargo test --workspace`（写引擎 target/ 与 /tmp），
   按 crate 归属拆为 per_suite 六套件（62 不变）；面板 12 与 §1/§3 同步；
   其余 13 份 data 逐字节未动（§4.1 未变清单）。原始输出存
   `/tmp/vizir-fix-baseline/cargo-test-round2.txt`。
5. **R5（P3）面板 10「diff 侧 2 + apply 侧 12」错**：VIZ-PATCH-0002 两侧都
   发射（diff_scene@patch.rs:79 与 apply_scene_patch@:138；apply 侧
   target≤base 校验在 :136-141），apply 路径可发射 13 码。**fixed**：改写为
   「0001 仅 diff；0002 两侧都查（diff@:79/apply@:138）；其余 12 在 apply
   侧」，0002 行锚点补 `:136-141`；§3 对应行同口径。
6. **R6（P3）examples 口径把未跟踪 WIP 计入**：工作树 11 = git tracked 10 +
   未跟踪 1（examples/mixed/capacity-planning.viz.yaml，engine.json 基线在案）。
   **fixed**：hero 补口径注（工作树 11 = 已跟踪 10 + WIP 1；fresh clone 复现
   以 10 为准），§1 数据面同步。examples.json 数据本身如实（rglob 工作树）
   不改。
7. **R7（P3）空 `<text>` 死标记致规则 partial**：panels.py 输出空文本元素
   （04 五处来自恒空 extra 行、08 一处来自 `("", INK)` 间隔行），04 跳
   125/378、08 跳 31/496 个检查对——修复前 `--require-complete` 04/08 均
   exit 1（已实测留证）。**fixed**：svgkit.text() 对空串零发射（占位行距
   保留）、extra 行加空值守卫；重建后 13 文件空 `<text>` 计数全 0，
   `--require-complete` 13 × exit 0。
8. **R8（P2）README 复现块 $V 未用**：设了 `V=` 但 5 条命令写裸 `vizir`
   （会解析到别处二进制）。**fixed**：6 条全部 `$V`；逐字实跑验证：explain
   六字段、manifest 174 条（首条 decision 逐键相等）、capabilities 全量
   JSON 相等、VIZ-CAP-0001 exit 1 且输出/manifest 零残留、双跑 lower
   byte 级一致且 sha == determinism.json 冻结值。
9. **R9（P3）README 目录树失真**：把 data/provenance.json 画成顶层文件。
   **fixed**：data/ 注明 14 份 JSON 含 provenance.json（在 data/ 内）。
10. **R10（P3）contract.md 章节号错位**：「双跑一致在 02 号面板」——可见
    章节号 02 = Origin，问责管线是 01（文件名 03-origin/02-pipeline 与章节
    号错位）。**fixed**：改为「01 号面板（问责管线；svg 文件名
    02-pipeline.svg，章节号与文件序号错位，以章节号为准）」。
11. **R11（P3）build.py 计数断言被结构性遮蔽**：全页子串存在性使 "7"⊂"174"、
    "11"⊂"110"。**fixed**：改 stat-tile 锚定形态断言（`>7</text>` 等 6 个
    锚定形态覆盖 7 个数字），README/§2 注明门禁口径。篡改自测（实测）：
    临时把 hero 子命令计数渲染改 7→8，新门禁 exit 1（`>7</text>` missing）；
    同一篡改页在旧口径下 "7" 命中 "174" 仍通过——遮蔽实锤。测毕还原重建。
12. **R12（P3）「每码首个出现 file:line」口径不实**：diag_codes.json 的
    first 字段是单次 `grep -rn` 的遍历序首见，非按 (file,line) 排序的真
    首个。**fixed（方案 a，侵入最小）**：§1 改口径为「grep 遍历序首个
    出现」并明示与排序首见的差别；固定树上同机重跑遍历序稳定，跨机不
    作承诺，故不改数据。
13. **R13（P3）§7 措辞与实况不符**：终态 8 行与 dirty_status_baseline
    **逐行全同**（基线快照时 docs/infographics/ 已存在），并非「仅多出
    ?? docs/infographics/」。**fixed**：§8 句子改为「终态与基线逐行相同，
    交付未改变引擎工作树任何其他状态」；§2 引擎只读行同步。
14. **R14（新增，修复 R7 时发现的第 14 项缺陷，超出仲裁 13 项范围，如实
    补记）**：panels.py 读 explain 字段用了下划线键名（`generated_by`/
    `data_key`/`mir_node`），而 CLI 实际输出连字符键（`generated-by`/
    `data-key`/`mir-node`，explain_samples.json 冻结即如此）——致 04 面板
    五张卡的 pass chip 全部渲染为「?」、左缘着色全部退化为 MUTED 灰、
    extra 信息行恒空，而面板 src_note 声称「generated_by 即着色：五个
    查询命中四条不同生成 pass」与渲染实况不符。**fixed**：改读连字符键；
    重建后 chip 显示四条真实 pass、五卡四色（FLOW/FLOW_DK/TEAL/CH_B）、
    三卡出现 data-key/data-lineage/mir 信息行；对抗验证 8 检查未覆盖此
    点，特此披露。

## 8. 引擎零改动复核

```bash
git -C ~/projects/plot/vizir status --porcelain
```

终态与 `data/engine.json:dirty_status_baseline` 逐行比对：**逐行相同**——
基线快照时 `docs/infographics/` 已存在（冻结发生在交付目录创建之后），
故终态 8 行与基线 8 行完全一致，交付未改变引擎工作树任何其他状态；
`git log` 无新 commit（HEAD 仍 `52840b8`）。构建/测试只写过 `target/` 与
`/tmp`（二轮修复同口径：cargo test 只写引擎 target/，验证命令只写 /tmp）。

## 9. 已知噪声与过滤方式

| 噪声 | 处理 |
|---|---|
| manifest 的 `output` 字段随输出路径变化 | 实测：同路径重跑 manifest byte 级一致；异路径 diff 仅 `output` 一键。冻结时固定沙盒路径，统计口径不含该键 |
| PNG 依赖外部 rsvg-convert/ImageMagick（版本敏感） | 不以 PNG 为证据锚：冻结 SVG+JSON sha；PNG 仅呈现层，且其 1 条 loss record 本身如实上图 |
| 引擎工作树既有脏项 | 基线快照冻结于 `data/engine.json`，终态比对（§8） |
| cargo test 输出的 `Running`/`test result` 分流 stderr/stdout | 冻结器合并流解析，`tests.json` 记录 per_suite 计数（二轮 R4 重取：首轮 per_crate 键缺 crate 名致三 crate 同名套件误并） |

## 10. 环境

macOS (Darwin 25.5.0, arm64) · chrome-headless-shell
`chromium_headless_shell-1234`（CDP，等 `document.fonts.ready` + 双 rAF，
`--force-color-profile=srgb`，`--hide-scrollbars`）· svg-linter
`~/projects/plot/svg-linter`（release 构建）· Python 3（Pillow）· Node（内置
WebSocket）。页面字体依赖本机 'Source Han Serif/Sans SC'，回退 PingFang SC；
异机重建若字体不同，位图指纹会变而 SVG 指纹不变。

## 11. 代码细节下页改版（2026-09-03，撤回不删除）

舰队政策（用户 2026-09-03 明令）：信息图**页面**（index.html + 全部 svg +
位图）零代码细节——引擎源码文件名、file:line 坐标、行区间、逐字源码摘录卡、
引擎标识符（含公开 API 函数/类型/变体/测试名）、引擎内部目录路径、生成器
自身文件名与重建命令，全部只留在记录层（data/*.json、provenance、本文件）。
页面引用层改为**稳定声明编号 E1–E6 + 「证据链见 VERIFICATION」**。本节为
该改版的完整记录；§1–§10 中与旧版面（逐字卡上页、file:line 上页）相关的
声明以本节为准，原文划线留档不删。

### 11.1 版面元素映射表（旧形式 → 新形式）

完整 39 条逐元素映射表（含每条承载的技术声明）冻结于改版工作档案
`/tmp/retro-vizir/mapping.md`（沙箱）；树内摘要如下。**不降级铁律**：每条
旧形式承载的声明均以新形式存活。

| 旧（上页） | 新（上页） | 承载声明 |
|---|---|---|
| 逐字源码卡①：`VizError::Diagnostic(format!("VIZ-CAP-0002: backend {:?} cannot lower: {failures}", self.backend))`（08 面板，含 `capability.rs:127-131` 注释行） | 编号伪代码卡「谈判破裂的唯一出口（伪代码）」：① 汇总全部不支持判决 ② 打成一条稳定诊断 VIZ-CAP-0002、点名后端与缺口 ③ 立即中止（不写输出/manifest、exit 1） | 谈判破裂 = 全部 Error 决策汇总成一条点名诊断 + 原子失败 |
| 逐字源码卡②：`ScenePatch { protocol_version… base_revision: Revision(7) … }`（10 面板，含 `patch.rs` 信封标题） | 「补丁信封六要素」卡片（协议版本 0.1 冻结值 / 文档标识 / 事务标识 / 基线版本 / 目标版本 / 操作列表；示例：基线 7 → 目标 8） | 信封六要素 + 双版本规则 |
| 四道门 `patch.rs:77-82 / 118-123 / 124-129 / 130-135` 锚点列 | 「拒绝门归属」列（0002 两侧都查；0003/0004/0005 apply 侧） | 门位置知识升级上页（坐标下页） |
| 11 面板 5 个英文测试名 + `patch.rs:457/501/530/573/667` | T1–T5 五条中文保证（主证/比对侧拒绝/执行侧拒绝/逐操作精确诊断/过期必拒） | 五项测试保证；主证补强为「严格相等」（§3 已核） |
| op 变体名 `RemoveNode a / ReplaceNode b: w=3 / InsertNode c @1 / ReorderChildren a,b→b,c` | 「删节点(键=a) · 改节点(键=b, 宽=3) · 插节点(键=c, 位置=1) · 子节点排序(a,b→b,c)」+ 顺序铁律一行 | op 序列精确顺序声明 |
| 03 面板 `struct Origin` + `crates/…scene.rs:132-142` 注释行 + 左表字段名 | 「六字段责任档案」中文领域名列（HIR 视图声明/MIR 标记组/数据键/数据谱系/生成 pass/人读解释）；原文键名仍在右卡 CLI 输出实录（产品 JSON 键，判可留） | 六字段全为真实值 |
| 12 面板出处表（冻结数据文件名 + `scene.rs:132-142` 等 5 行坐标） | 「声明登记簿」E1–E6（编号 · 一句话声明 · 覆盖章节）+ 指针行 | 出处功能升级为声明编号登记簿 |
| 其余 src_note 中的 file:line / 函数名（`capability.rs:134/152`、`diff_scene`、`apply_scene_patch`、`cli.rs`、`main.rs:334`、`crates/ 全量 grep`、`BTreeSet` 等，共 26 处坐标/32 处文件名） | 一律改为「声明 Enn · 证据链见 VERIFICATION」式指引；函数语义中文化（先逐节点收集能力要求，再逐节点谈判判决；按「节点 × 能力项」去重） | 全部机制声明原文存活 |
| 页脚 `重建：python3 build.py…`、`数据冻结于 prep_data.py 一次运行` | 「重建命令与验收管线见 README / VERIFICATION」「证据冻结于一次真实引擎运行」 | 政策 §6（生成器名与重建命令下页） |

判定留痕：产品输出值与用户 I/O **保留**上页——生成 pass 名、能力项名
（paint.alpha…）、稳定诊断码（VIZ-*，stderr 原文）、JSON 线格式键（输出
实录卡内）、`service-health.viz.yaml` / `scene-patch.schema.json` /
`run-a.scene.json` / `run.png.manifest.json`；已发表算法/标准名同判可留。
交付冻结数据文件名（explain_samples.json 等）作为**引用层**不再上页（数字
与值本身仍程序读取自冻结数据）。

### 11.2 清扫证据（六式，独立脚本复扫）

改版前基线（独立 python 正则脚本，`/tmp/retro-vizir/sweep-baseline.txt`）：
file:line 56（svg 源内 26）· 引擎源码文件名 64（svg 源内 32）· 行区间 20
（svg 源内 10，均为真行区间，无时间/版本号误报）· 第N行 0 · 源码关键字 6 ·
调用串 4（`Revision(7/8)`，真标识符）· base64 原始命中 0（误报剔除计 0）。
元素级清单 79 行存 `/tmp/retro-vizir/elements-baseline.txt`。

改版后（同脚本，`sweep-after.txt`）：**六式全部 0**；build.py 内建
code_detail_gate 同口径拦零（26 个引擎源码文件名快照 = `git ls-files` 代码
扩展名实测清单 + 交付树生成器文件名；25 个禁用标识符；10 个新形式
needle；E1–E6 覆盖；VERIFICATION 指引 ≥12 处）。

### 11.3 断言层变化（只增强不减弱）

- 保留：面板数 12；6 个锚定形态覆盖 7 个关键计数（`>N</text>`）；4 项自污染
  检查（`<script`/`src="http`/`@import`/`fetch(` 全 0）。
- 新增：code_detail_gate 六式零命中 + 文件名/标识符黑名单零命中（见 11.2）；
  新形式 derived needle 10 条（伪代码卡标题与诊断命名、信封六要素与双版本
  规则、顺序铁律、严格相等、中文套件标签「核心库 27」、专项测试计数、声明
  登记簿）；E1–E6 逐个存在断言；VERIFICATION 指引计数断言。
- 语义迁移：原断言逐字 needle 的载体（逐字卡、file:line）改断言新形式
  needle，旧 needle 对应内容已全部下页（本节留档）。

### 11.4 指纹迁移表（改版轮：旧 → 新，一行一由）

| 产物 | 旧 sha256 | 新 sha256 | 变更由 |
|---|---|---|---|
| index.html | `2c109272…` | `d7cd10cb…` | 全部面板与页脚改版（下列各行） |
| svg/01-hero | `23b6bd0b…` | `a21a6241…` | 卡 3 换形 + src_note 三条下页 |
| svg/02-pipeline | `bd2016f0…` | `fe729f61…` | src_note 两条下页 |
| svg/03-origin | `4373d91d…` | `76022419…` | struct 头/坐标/字段名列 → 领域名 |
| svg/04-explain-tree | `252d5851…` | `1b711157…` | src_note 数据文件名改声明编号 |
| svg/05-coverage | `c1519756…` | `35ba3b58…` | 同上 |
| svg/06-capability-surface | `8ac12061…` | `a67ae51c…` | 副题/元行/策略行领域化 + 函数名下页 |
| svg/07-decisions | `60a5d7b8…` | `a16729d5…` | BTreeSet/坐标下页 |
| svg/08-fail-loud | `60c852ee…` | `e547e2d5…` | 逐字源码卡①→伪代码卡 |
| svg/09-loss | `c44dcf6c…` | `33b024a4…` | src_note 下页（cli.rs/verify_png_alpha 移记录层） |
| svg/10-patch-gate | `3775e79f…` | `6dbeb2a1…` | 逐字源码卡②→信封卡 + 门表归属列 |
| svg/11-patch-equivalence | `2e48718a…` | `42099ca1…` | 流程框/op chips/测试表领域化 |
| svg/12-gates | `2857e29a…` | `e9dd9717…` | 防漂移行/套件标签/出处表→登记簿 |
| svg/99-footer | `44738feb…` | `872c5689…` | 页脚重建命令与生成器名下页 |
| render/full@2x.png | `f508ed3c…` | `368f87bf…` | 重拍（尺寸不变 2400×17116；页面 CSS 高 8558 未变） |
| render/full@2x.gray.png | `5d128464…` | `2608c7bf…` | 同上（灰度版） |
| render/thumb.png | `eda53bca…` | `bc8ee3f2…` | 同上（600×4279） |

未变：14 份 `data/*.json` 逐字节不动（本轮零追加零修改）；shoot.js / stitch.py /
svgkit.py / prep_data.py 未改。完整新指纹全值：index `d7cd10cb
b2b21ef2a22f7911d4a59e690231fb08c5ea4e997df5b86572b5f292`，面板 svg 见
`shasum -a 256 svg/*.svg` 可复算。

### 11.5 记录层冻结（自页面撤下的原文，锚点仍按 §3 逐条有效）

1. 08 卡①原文（引擎 `capability.rs:127-131`）：`VizError::Diagnostic(
   format!("VIZ-CAP-0002: backend {:?} cannot lower: {failures}", self.backend))`。
2. 10 卡②原文（`patch.rs`，测试样例值）：`ScenePatch { protocol_version:
   "0.1", document_id: "doc", transaction_id: "transaction/test",
   base_revision: Revision(7), target_revision: Revision(8), operations: [ … ] }`。
3. 11 面板 5 测试名与行号（冻结于 `patch.json`）：
   `diff_and_apply_match_full_scene_semantics`@457、
   `diff_rejects_cross_document_and_non_advancing_revisions`@501、
   `apply_rejects_foreign_patches_and_non_advancing_revisions`@530、
   `apply_rejects_each_malformed_operation_with_its_exact_diagnostic`@573、
   `revision_mismatch_rejects_patch`@667；顺序注释 `patch.rs:473`
   "Removals first, then next-order replace/insert, then reorder"。
4. 防漂移测试（`schemas.json` 冻结）：`crates/vizir-cli/tests/cli.rs:312
   schema_subcommand_emits_the_checked_in_canonical_schemas`。
5. fail-loud 同断言测试：`cli.rs
   png_render_without_a_rasterizer_fails_without_partial_output`；
   alpha 把关：`cli.rs verify_png_alpha`（VIZ-ARTIFACT-0001/0002/0003）。
6. op 变体名：`RemoveNode / ReplaceNode / InsertNode / ReorderChildren`。

### 11.6 声明编号证据链表（E1–E6；页面引用层的登记侧）

| 编号 | 声明（一句话） | 覆盖章节 | 冻结数据 | 源码锚点（记录层） |
|---|---|---|---|---|
| E1 | 逐节点溯源：六字段责任档案 + explain 六查询 + 覆盖 100% | 02·03·04 | scene_nodes.json、explain_samples.json | `scene.rs:132-142`（struct Origin）、省略规则 `:136/138` |
| E2 | 能力谈判：174 条逐节点判决 + fail-loud 原子失败 | 05·06·07 | capability.json、fail_loud.json | `capability.rs:134/152`（要求收集/谈判）、`:127-131`（VIZ-CAP-0002 汇总）、`main.rs:334`（VIZ-CAP-0001） |
| E3 | 补丁等价：信封六要素 + 四道拒绝门 + 5 项测试保证 | 09·10 | patch.json、schemas.json | `patch.rs:64/113`（diff/apply）、门区 `:77-82/118-123/124-129/130-135/136-141`、测试 `:457-667`、顺序注释 `:473` |
| E4 | 确定性地基：三对产物指纹全等（配角） | 01 | determinism.json、engine.json | 数据内冻结 sha（normalize/lower/render 双跑） |
| E5 | 损耗诚实：1 条损耗记录逐字落 manifest | 08 | capability.json（png_losses/rasterizer）、determinism.json（manifest 噪声） | manifest 键 losses/rasterizer 实测 |
| E6 | 门禁与指纹：81 码 ×14 族 + schema 防漂移 + 62 测试 | 11 | diag_codes.json、schemas.json、tests.json | `crates/` 全量逐码枚举（grep 遍历序，§7/R12）、`tests/cli.rs:312` |

### 11.7 改版轮门禁与复核实录

- build（/tmp 平面拷贝，`PYTHONDONTWRITEBYTECODE=1`）：rc=0，断言全绿
  （6 锚定计数 + 10 新形式 needle + 4 自污染 + 六式清扫 + 黑名单 + E1–E6
  覆盖全零/全命中）。
- svg-linter：13 文件 `check --plain` 全 exit 0、findings 0；`--require-complete`
  13 × exit 0（完整 TSV 留 `/tmp/retro-vizir/lint/`）。
- 渲染三重断言：页面宽 1200 ✓；stitch 位图 2400×17116 == 8558 CSS 高 × 2 ✓；
  14 裁片逐面板目检无重叠/裁边/越界/断词（01/03/06/07/08/09/10/11/12 及页脚
  逐张目检；02/04/05 仅 src_note 文案替换，linter 兜底）。
- 双跑确定性：连续两次 build `cmp` byte 级一致；真空跑（重建目录不带旧
  index.html/svg 产物，带齐 build 全部输入：panels/svgkit/data）rc=0 且产物
  `cmp` 一致（§5 同口径）。
- 引擎仓只读复核：改版开工 HEAD `c7f0a896`（注意：冻结时 HEAD 为
  `52840b8`，冻结后引擎自行前进，与交付无关）；改版仅触碰
  `docs/infographics/vizir-explainer/` 内文件，零 commit、零树外改动。

# VizIR 演进计划：让已有契约真正可配置、可执行

原始评估日期：2026-10-04。原始源码基线：[4830efe](https://github.com/junix/vizir/commit/4830efe7c2b9530f9d2a960c5c03be11c4f213ee)。下方第 1–6 节保留原始评估与阶段设计，包含当时尚未发布的描述；当前交付状态以本节及第 7 节为准。

## 当前交付状态（2026-10-04）

- 已发布：guide 显式 scale 引用 [19208ff](https://github.com/junix/vizir/commit/19208ff7770f8c86bd9ba6851fc0ebfb1aa9e1c6)、独立的微小数值域修复 [def3e52](https://github.com/junix/vizir/commit/def3e520a22b4cad48bcb4c7ea1139bb76cf225c)、可选 0.2 数值轴格式 [351323e](https://github.com/junix/vizir/commit/351323e18ae046dc656744b3e6fa0498615c589c)
- graph-ir 的可选 OriginMapV1 已独立发布：[2e2fc8b](https://github.com/junix/graph-ir-rs/commit/2e2fc8b2f2eefee9c26ebc70e709f6da4d1c55b1)；它不是 VizIR 格式功能的依赖，也不合并领域 IR
- 已发布：后端既有 Scene2D 身份别名 / limits 执行检查 [19c9b6e](https://github.com/junix/vizir/commit/19c9b6e50e719dfa66d39df7c157ac894b6ec65e)；没有新增 Scene 版本协议或 clip 模型
- 类型化组合布局、样式 token/patch、数据物化，以及完整 Scene/ScenePatch 验证仍是后续计划

0.1 保持新读取器的 wire 可读性；显式数值格式使用 0.2。当前有界验证不代表所有仓库、所有数值范围、字体或视觉场景均已验收。历史源码探针不再用于断言已修复路径仍有原缺陷。

## 1. 保留分层，先修可执行性

继续采用：

```text
VizHIR：可编辑的语义源
    → VizMIR：类型、数据绑定、比例尺、guide、布局请求
    → Scene2D：已解析的几何、样式、顺序与来源
    → capability 检查 → SVG / PNG
```

HIR 保持权威来源，MIR 与 Scene2D 可重新生成。不要把交互、动画、3D、任意脚本或后端对象塞入静态 SceneNode。现有[分层与升级原则](https://github.com/junix/vizir/blob/4830efe7c2b9530f9d2a960c5c03be11c4f213ee/docs/ir-family.md)仍然适用。

## 2. 已核实：主要限制在哪里

| 范围 | 已有能力 | 当前限制与影响 |
| --- | --- | --- |
| 布局 | 显式 frame、几何组变换、确定性图布局、共享图表标题布局 | 仪表盘必须手工分配绝对 frame；轴刻度数量和边距固定，刻度文本不参与空间分配 |
| guide | MIR 已有 guide ID、scale 引用与方向 | 实际刻度按比例尺 ID 的 `/x`、`/y` 等后缀查找；合法重命名可导致刻度消失 |
| 数值显示 | 线性比例尺与有限数值检查 | 格式器只有 k/定点策略，大数会产生极长标签；数值有效不等于文字可读 |
| 样式 | 类型化图元样式、组透明度、默认值展开 | 没有主题 token、命名样式或样式继承；图表外观仍含大量编译器常量 |
| 数据与表达式 | 类型化 AST、字段类型、稳定数据键、绑定引用 | 数据算子只有 Inline；表达式未执行，Scene 构造直接使用已物化的 mark 数据 |
| 来源与编辑 | HIR/MIR ID、data key、lineage、ScenePatch | 部分 guide 来源只指向图表；图边 ID 依赖列表位置；补丁检查尚不等于完整 Scene 验证 |
| 扩展与兼容 | 严格 HIR/MIR、生成 schema、版本字段 | 只验证 0.1；Scene 的部分嵌套对象仍会接受并丢弃未知字段；没有已实现的迁移命令 |
| 后端 | 按节点报告 feature 与 fidelity | capability 中的 accepted_ir 和 limits 未实际约束协商；PNG 仍是 SVG 栅格化路径 |

对应源码：[HIR](https://github.com/junix/vizir/blob/4830efe7c2b9530f9d2a960c5c03be11c4f213ee/crates/vizir-core/src/hir.rs)、[MIR](https://github.com/junix/vizir/blob/4830efe7c2b9530f9d2a960c5c03be11c4f213ee/crates/vizir-core/src/mir.rs)、[lowering](https://github.com/junix/vizir/blob/4830efe7c2b9530f9d2a960c5c03be11c4f213ee/crates/vizir-compiler/src/lower.rs)、[Scene 构造](https://github.com/junix/vizir/blob/4830efe7c2b9530f9d2a960c5c03be11c4f213ee/crates/vizir-compiler/src/scene_builder.rs)、[capability](https://github.com/junix/vizir/blob/4830efe7c2b9530f9d2a960c5c03be11c4f213ee/crates/vizir-core/src/capability.rs)。

针对性运行核实了以下行为，不应将其描述成已经支持的灵活性：

- 重命名 x 比例尺并更新 mark、guide 引用后，MIR 验证成功，x 刻度却消失
- 将 y 表达式改成 `y * 2`，或修改 MIR inline 源数据，验证仍成功，但 Scene 不变
- Scene 根对象和 Origin 中的未知字段可被读入，重新序列化时被丢弃
- capability 声明不匹配的 IR 和零节点上限，仍可能接受该 Scene
- ScenePatch 设置负宽度，仍可通过当前补丁应用检查

这些探针使用前一提交 b62be8e9 的源码；4830efe 仅增加原始数值跨度防护及相应测试/说明，不改变上述实现路径。它们是定点行为核实，不代表全仓测试结果。

## 3. 第一阶段：guide 引用与数值轴格式

### 3.1 独立修复 guide 引用

按 `MirGuide.scale` 解析比例尺，不再从字符串后缀猜测。刻度、标题与显式图例服从对应 guide。

- 保持 HIR 0.1 和 MIR 0.1 的 wire 结构
- 测试合法重命名、误导性后缀、缺失引用、错误比例尺类型与歧义配置
- 沿用数值舍入容差检查 guide 范围与绘图区的关系，不放松缓存一致性规则
- 不在本次修复中承诺次坐标轴或任意手写 MIR range

本次采用的兼容规则：当前 line/bar 会绘制图例，却未生成对应的 legend guide。因此，存在显式 legend guide 时以它为准；缺失时保留 0.1 的 mark-color 隐式图例回退。本次不改 HIR、schema 或 normalization，不新增 0.1 guide 数组项，已有规范化 MIR 字节和图例可视结果均作为回归目标。完整显式 guide 输出留待后续 0.2 normalization 增强。

### 3.2 新增可选、类型化的数值格式

建议的 HIR 0.2 写法：

```yaml
version: "0.2"
# 放在完整 chart.scatter 或 chart.line 视图中
x:
  field: sample
  axis:
    number_format: {notation: scientific, precision: 2}
y:
  field: signal
  axis:
    number_format: {notation: fixed, precision: 3}
```

先只提供 scientific 与 fixed，precision 明确表示小数位数，并设定有限上限。bar 的 value 轴采用相同契约；分类轴和 ordinal 图例不应接受然后忽略数值格式。上例是计划接口，最终以发布的 schema 和可执行示例为准。

格式与布局必须一起完成：

1. 数值格式归属 HIR/MIR guide，编译时解析为 Scene2D 文本，不交给 SVG/PNG 自行解释
2. 未设置格式时保留旧格式器与已有默认可视结果；舍入后的负零不显示为负数
3. MIR normalization 与 Scene 构造使用同一个刻度文本及空间分配过程，避免重算分歧
4. 用确定性文本包络计算 y 轴边距、x 轴端点和相邻标签间距；单位是 scene-unit，包络不是字体整形保证
5. 显式 fixed 产生过长标签、科学计数精度不足以区分刻度或 frame 过窄时，给出可操作诊断。不得默默截断、缩小、隐藏标签或改换用户选择的格式
6. 不把数值格式当成任意大坐标的支持承诺；既有有限性、跨度、布局与 PNG 资源限制继续生效

建议公开示例：科学量级的正负大数、小数测量精度、同图不同轴格式。验收覆盖 `±1e150`、小数、零/负零、精度边界、重名/近邻刻度、最小可用 frame、不可容纳布局，以及 HIR → MIR JSON → Scene2D 的一致性。渲染产物仍须检查真实透明度与交付尺寸下的可读性。

## 4. 明确 0.1 / 0.2 兼容边界

这是新格式能力的建议版本策略：

兼容保证覆盖旧 wire 的可读性和既有正常输入的默认行为。已确认的数值推断错误应作为独立 bugfix 提交、测试并记录，而不是通过“是否设置格式”隐式切换比例尺策略。格式决定文本，数值域推断保持独立。

- 新读取器继续接受旧 HIR/MIR 0.1
- 0.1 未使用新能力时，不新增格式字段，不改变旧数值格式和默认绘图结果
- 格式字段使用缺省值及“缺失则不序列化”规则；输出 `null` 也会被旧严格读取器当作未知字段拒绝
- 0.1 加新格式字段必须报版本诊断；使用者显式选择 HIR 0.2
- 带新语义的 HIR 0.2 输出 MIR 0.2，并保留独立的 source_hir_version
- MIR 0.1 不得携带 0.2 格式语义；新读取器同时验证版本和字段适用范围
- 旧读取器应拒绝新字段或新版本。不能把“新程序能读旧文件”说成“旧程序能读新文件”
- schema 漂移测试、运行时验证和文档共同覆盖版本边界；不要宣称存在尚未实现的迁移命令

新增公开 Rust struct 字段也可能影响外部 struct literal；wire 兼容与 Rust 源码兼容须分别说明。本次 guide 引用修复不改变 0.1 规范化结构；后续若在 0.2 补齐显式 guide，应另行说明和测试。

## 5. 后续阶段：按真实使用场景推进

### 阶段二：类型化布局与样式

增加有限的 grid/stack 组合布局，先解析成既有 frame；绝对 frame 保留。增加主题 token 与局部覆盖，在进入 Scene2D 前全部解析。

样式覆盖必须采用独立的可选 patch 类型，明确“未填写”和“显式填写默认值”的区别。现有 ShapeStyle 已将 stroke_width、opacity 缺省为实值，不能直接用于正确的继承。先服务已有 mixed dashboard 场景，再考虑组件/符号复用。

### 阶段三：统一数据物化语义

先提供一个确定性的执行/物化过程，再添加 filter、calculate。数据源、表达式、比例尺与 mark 实例必须由同一过程生成或验证，避免源数据改了而缓存实例不变。

必须定义 null、数值溢出、除零、顺序、稳定键和 lineage；禁止任意字符串脚本与隐式 IO。aggregate、join、分面和重复实例计划，待身份及空结果行为明确后推进。图表数据变换与图拓扑投影保持不同的领域语义。

### 阶段四：强化交换、来源与后端检查

- 构造 Scene 和应用补丁后执行完整验证，覆盖尺寸、有限性、样式、身份与几何一致性
- 核心字段一律严格；确有需要时才引入有命名空间、有保留规则的可选元数据扩展
- 检查后端接受的 IR/版本和已声明限制；未知能力保持 unknown，不能当成支持
- guide、图边和生成图元记录精确来源；稳定语义 ID 不依赖列表顺序或 SVG DOM ID
- 对任何近似、丢弃或栅格化保留明确 loss 记录；不把 profile 声明等同于已执行的 fallback

## 6. 跨仓协调：OriginMap，不合并领域 IR

graph-ir 可另行提供可选 `OriginMapV1` sidecar，将 primitive ID 映射到语义节点/边及可选源码位置。边被拆成多个路径段时，每段仍映射到同一条语义边。新函数或报告 wrapper 可承载此映射，默认 JSON/SVG 不应被无提示改变。

VizIR 与 graph-ir 共享来源引用、诊断和 capability 的少量约定即可。VizHIR/MIR/Scene2D、图的 Semantic/Diagram/Layout/Render，以及语言 AST 继续各自承担职责。OriginMap 是后续协作计划，不是本次 VizIR 格式功能的依赖或已完成成果。

每个阶段独立提交、独立验收。验收包括针对性失败测试、现有仓库检查、受影响复杂示例和最终图像检查；不得仅因类型或字段已存在，就把能力标为可用。


## 7. 已发布交付记录与验收边界

### 7.1 guide 引用与独立数值修复

[19208ff](https://github.com/junix/vizir/commit/19208ff7770f8c86bd9ba6851fc0ebfb1aa9e1c6) 已按 `MirGuide.scale` 解析刻度、标题与显式图例，并诊断缺失引用、类型错误和歧义。10 个回归测试通过，旧实现有 9 个失败；该阶段 148 个测试通过，11 个示例的 MIR、Scene2D、SVG、PNG 共 44 份字节比较保持一致。0.1 没有显式 legend guide 时仍保留 mark-color 回退；没有无提示增加 0.1 guide 项。

[def3e52](https://github.com/junix/vizir/commit/def3e520a22b4cad48bcb4c7ea1139bb76cf225c) 独立修复真实常量判断、次正规数 nice-step 下溢和向外覆盖原始极值的边界。24 个 tiny-domain 案例覆盖 scatter/line/bar 的有限域、刻度及有意义的几何区分；44 份旧示例比较保持一致。数值域推断不由是否选择格式来切换。旧格式仍可能把很小的数显示为 0；通用极大常量 nice bounds 不在这项修复内。

### 7.2 0.2 数值轴格式已发布

[351323e](https://github.com/junix/vizir/commit/351323e18ae046dc656744b3e6fa0498615c589c) 的最终接口见 [wire contract](https://github.com/junix/vizir/blob/351323e18ae046dc656744b3e6fa0498615c589c/docs/wire-format.md) 与 [MIR schema](https://github.com/junix/vizir/blob/351323e18ae046dc656744b3e6fa0498615c589c/schemas/viz-mir.schema.json)：

- `fixed` 和 `scientific` 的 precision 表示小数位数，接受 0..12 的整数数值；schema 与运行时对整数数值写法保持一致，拒绝非整数及越界值
- scatter/line 的数值轴及 bar value 轴可显式选择格式；分类轴、ordinal 图例等不适用位置不能接受后再忽略
- HIR/MIR 0.1 继续可读；新格式语义必须显式选择 0.2，保留独立 `source_hir_version`。旧严格读取器不会因此自动理解 0.2
- 同一实际刻度文本用于 MIR 与 Scene 布局；舍入后的负零归零，重复或不可表示的标签、过长标签和放不下的 frame 给出诊断，不静默裁剪、缩小或换格式
- 新公开 Rust 字段可能要求外部 struct literal 增加字段；wire 兼容不等于 Rust 源码兼容。没有迁移命令

最终 `just check` 为 **191 通过、1 个有意忽略的子进程 fixture**；28 个编译器集成测试、300 个小数 frame 往返案例、76 个 CLI 拒绝组合、19 个 schema 案例通过。14 个示例生成 56 个产物，旧 11 个示例共 **44 份 MIR/Scene2D/SVG/PNG 字节保持一致**；14 个 PNG 透明度检查通过。三个新示例（科学量级、测量精度、混合轴格式）已按交付尺寸检查，原有 11 张 gallery 卡片保留。Windows/macOS 为编译检查，并非原生运行验收。

文本包络是确定性的 scene-unit 估计，不是通用字体整形保证。显式格式不承诺任意数值范围都可绘制，也不改变资源限制。此前跨库 65 案例报告保留 5 个大数标签可读性失败；本次只为 VizIR 指定的 opt-in 示例提供后续格式验收，没有重写历史报告或修复所有 Go/Rust/default 标签。

### 7.3 Graph OriginMapV1 已独立发布

[graph-ir 2e2fc8b](https://github.com/junix/graph-ir-rs/commit/2e2fc8b2f2eefee9c26ebc70e709f6da4d1c55b1) 在 primitive 生成处记录可选、类型化的 graph/node/edge/group/callout 引用。默认 IR struct、wire 与 SVG 输出不变；拆分的边段保持同一语义边来源。未知字段、重复字段和非法 ID/reference 被拒绝。

`validate.sh` 通过 **120 个测试及 1 个 doctest**，包含严格 Clippy、格式、管线和序列化验证；严格 rustdoc 也通过。23 个管线输出目录的 **200 个默认产物**与基线字节相同，默认 lowering 的分配次数和分配字节数也相同。两份带 namespace 的 SVG 示例使用既有 primitive 属性连接到 sidecar，覆盖 26 个 primitive。

这只验证 **ID/reference 一致性，不证明来源真实性、源码修订身份或内容指纹**。sidecar 不复制源文本、源码位置、扩展/property 内容；backend 根、canvas 和共享 marker 定义不在范围内，arrow use 继承路径来源。完整边界见 [Origin sidecar verification](https://github.com/junix/graph-ir-rs/blob/2e2fc8b2f2eefee9c26ebc70e709f6da4d1c55b1/docs/verification/origin-sidecar.md)。没有新增或恢复远程 CI workflow；浏览器/栅格验收不包含在该 sidecar 记录中。

### 7.4 后端既有 IR 身份与 limits 已强制检查

[19c9b6e](https://github.com/junix/vizir/commit/19c9b6e50e719dfa66d39df7c157ac894b6ec65e) 按精确且大小写敏感的 `scene2d` / `scene2d-through-svg` 别名接受当前静态 Scene2D；其他 IR、带空格/前缀或 `scene2d@0.2` 等拼写不能靠 feature 名相同获得接受。Scene2D 没有序列化版本字段，profile 的 version 是后端实现版本；因此这不是版本协商协议，HIR/MIR 0.2 仍生成既有 Scene 结构。

`max-nodes` 是包含空 group 和所有递归子节点的 SceneNode 出现次数；重复 ID 不减少计数。上限包含边界，缺失没有该 profile 限制，零是真正的零。已声明的 `max-clip-depth` 当前测量为零，因为 Scene2D 没有 clip 构造；33 层普通 group 也不是 33 层 clip。本次没有新增 clip 模型、group 深度上限或完整 Scene 验证。

未知 limit key 返回明确错误；直接把 JSON 解码为 BackendCapabilities 时拒绝重复 limit key。如果调用者先解析成 serde_json::Value，重复 key 已被上游折叠，之后无法恢复。limit 仍用既有 u64 解码器，因此 2.0 这种浮点拼写仍不接受，尽管 JSON Schema 接受整数值 number；这与新轴格式 precision 的数值接受规则不同。完整边界见 [backend capabilities](https://github.com/junix/vizir/blob/19c9b6e50e719dfa66d39df7c157ac894b6ec65e/docs/backend-capabilities.md)。

原生 workspace 为 **207 通过、1 个既有忽略 fixture**，另有 **9 个独立边界检查通过**；严格格式/Clippy 和 Windows/macOS 编译检查通过。14 个示例的 **56 份 MIR/Scene2D/SVG/PNG 产物及 28 份 manifest**保持字节相同。失败决策让 is_accepted 为 false，require_accepted 返回 VIZ-CAP-0002；report 策略不能授权越过限制。CLI 使用内置 profile；自定义 profile 的 API 调用者也必须在输出前要求 accepted。这是在已经生成的 Scene 上执行检查，不是总内存上限。

### 7.5 尚未交付的阶段

第 5 节的组合布局、样式 patch、物化算子、完整 Scene/ScenePatch 验证，以及显式版本化 Scene envelope、更广的来源/交换约定仍需分别实现与测试。已交付数值格式和 sidecar 不能替代这些阶段；也不代表所有 49 个相关仓库已经完成验收。

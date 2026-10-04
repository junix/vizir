# VizIR 演进计划：让已有契约真正可配置、可执行

评估日期：2026-10-04。源码基线：[4830efe](https://github.com/junix/vizir/tree/4830efe7c2b9530f9d2a960c5c03be11c4f213ee)。

本文区分**已核实的现状**与**计划中的能力**。guide 引用修复和数值轴格式正在实现，本文不代表它们已经发布或通过全部验收。其余阶段是建议顺序，不是已交付清单。

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

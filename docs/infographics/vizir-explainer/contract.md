# 视觉契约 — vizir 技术长图

- 受众：后端 / 编译器 / 可视化工具方向的工程师读者；对「文档编译器」有直觉但没读过 vizir 仓库的人。
- 一句话论点：vizir 把可视化文档当**编译器问责对象**——每个 Scene2D 节点携带完整责任链可被 `explain` 质询，每次后端编译都逐节点做 capability 谈判（不支持即 VIZ-CAP 稳定诊断码 fail-loud，绝不静默消失），每条 ScenePatch 局部补丁都受 revision 校验并与全量重算语义等价。
- 读者问题：
  1. 「这个像素为什么在这」能不能问到数据行？（机制一：Origin 责任链 + explain 决策树）
  2. 后端给不了的能力去哪了？（机制二：能力面谈判 + 174 条逐节点 decision + fail-loud 原子失败）
  3. 局部补丁凭什么敢信？（机制三：revision 四道拒绝门 + diff/apply 与全量重算等价性测试）
  4. 降级与损耗诚实吗？（PNG 唯一 1 条 loss record 逐字落 manifest；证据锚 SVG+JSON 指纹）
  5. 以上凭什么不漂移？（81 稳定诊断码 ×14 族、schema 防漂移、62 测试、双跑 sha）
- **差异化边界（红线）**：姊妹图 graph-ir-rs-explainer 已占「五个版本化 IR 分层 + 可替换布局引擎 + 确定性布局算法」，plot-go-explainer 已占「跨机器逐字节渲染确定性」。本图**不以**「又一套分层 IR 编译器」或「渲染确定性」立意：字节级双跑一致仅作为问责地基在 01 号面板以配角呈现（可见章节号 01 · 问责管线，对应 svg 文件名 02-pipeline.svg——章节号与文件序号错位，以章节号为准）；本图全部立意钉在「逐节点 explain 溯源、逐节点 capability 谈判 fail-loud、revision 化 patch 等价性」三机制上。
- 叙事顺序：hero（论点+档案数字）→ 01 管线总览（每级留下什么责任证据）→ 02 Origin 六字段解剖 → 03 explain 决策树（六查询含错误路径）→ 04 覆盖面直方图 → 05 能力面名片 → 06 174 次判决（svg vs png）→ 07 fail-loud 原子失败实验 → 08 诚实的损耗 → 09 revision 四道门 → 10 补丁等价性 → 11 门禁与来源 → colophon。
- 媒介与尺寸：1200 CSS px 宽长图，12 个 SVG 面板 + 页脚，@2x PNG 渲染（chrome-headless-shell CDP，自 y=0 全宽 3600px 定高切片顺序拼接）；零 JS、零外链、图片 data-URI 内嵌。
- 语言：简体中文正文；~~代码符号 / 命令 / 路径 / 诊断码保留英文原文~~【2026-09-03
  改版勘误：页面零代码细节——源码符号/文件名/路径/行号/逐字摘录一律下页（政策
  「code detail stays off the page」）；保留的是产品界面：CLI 动词与参数、命令
  会话实录、稳定诊断码（VIZ-*）、输出实录卡内的 JSON 线格式键；页面引用层为
  声明编号 E1–E6 + 「证据链见 VERIFICATION」，见 VERIFICATION.md §11】。
- 配色：舰队 house style（PAPER #F7F4EE / INK #17212B / FLOW #356A79 蓝系主导），角色色 + ≤3 series 色，字面 hex 全部进 svgkit.py；用户偏好蓝。
- 每个数字的来源：真实 CLI 运行 / 源码行号 / 测试计数，冻结于 data/*.json + provenance.json（prep_data.py 一次性冻结，交付目录内永不重跑）；不存在的数字不上图。源码坐标只存记录层，页面以声明编号 E1–E6 引用（2026-09-03 改版）。

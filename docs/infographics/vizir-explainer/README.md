# vizir 技术长图（编译器问责对象）

可审计技术长图：12 个 SVG 面板讲清 vizir 的三个问责机制——逐节点 explain
溯源、逐节点 capability 谈判 fail-loud、revision 化 patch 与全量重算等价。
页面可见的每个数字都冻结自引擎真实运行（`data/*.json`），构建带内建断言。

## 目录

```
vizir-explainer/
├── prep_data.py      # 证据冻结器（已运行一次，交付内永不重跑）
├── data/             # 14 份冻结 JSON（含 data/provenance.json：引擎 commit、
│                     #   行号索引、冻结方法；provenance 在 data/ 内，非顶层）
├── svgkit.py         # SVG 基元 + 字面 hex 配色（house style）
├── panels.py         # 12 个数据驱动面板
├── build.py          # 拼装 index.html + 断言（面板数/关键计数/自污染/
│                     #   代码细节六式清扫拦零，见 VERIFICATION §11）
├── svg/*.svg         # 逐面板 SVG（门禁对象）
├── shoot.js          # chrome-headless-shell CDP 截图（y=0 起全宽 3600px 切片）
├── stitch.py         # 顺序拼接 + 位图高==页面CSS高×2 断言 + thumb/gray/裁片
├── render/           # full@2x.png / thumb / gray / 逐面板 1:1 裁片
├── index.html        # 最终长图（自包含，零外链零 JS）
├── contract.md       # 视觉契约（读者问题 + 差异化边界）
└── VERIFICATION.md   # 证据、门禁、指纹、修正记录
```

## 重建（逐字照抄可跑）

前置：引擎已按仓库 justfile 构建（`cargo build --release -p vizir-cli`，
本交付不重新触发）；`data/` 已冻结，重建**不**触碰引擎与 /tmp 沙盒。

```bash
cd ~/projects/plot/vizir/docs/infographics/vizir-explainer

# 1) 构建页面（内建断言：12 面板 / 7 个关键计数以 6 个 stat-tile 锚定形态
#    `>N</text>` 断言——裸子串会被 174/110 等遮蔽 / 4 项自污染全零 /
#    代码细节六式清扫零命中 + 标识符黑名单 + 新形式 needle + 声明编号
#    E1–E6 覆盖，2026-09-03 改版新增，详见 VERIFICATION.md §11）
python3 build.py

# 2) 重建确定性：连续两次输出必须 byte 级一致
python3 build.py && cp index.html /tmp/run1.html && python3 build.py \
  && cmp /tmp/run1.html index.html && echo IDENTICAL

# 3) SVG 门禁：13 个文件全部 exit 0 且 0 条 finding（TSV 的 outcome 行不算）
for f in svg/*.svg; do
  ~/projects/plot/svg-linter/target/release/svg-linter check --plain "$f"
  echo "$f exit=$?"
done
# 严格口径：
for f in svg/*.svg; do
  ~/projects/plot/svg-linter/target/release/svg-linter check --plain "$f" \
    | grep -c '^finding'
done   # 期望全 0

# 规则完整性口径（空 <text> 等死标记会让部分规则 partial 跳过）：
for f in svg/*.svg; do
  ~/projects/plot/svg-linter/target/release/svg-linter check --plain \
    --require-complete "$f" > /dev/null || echo "INCOMPLETE $f"
done   # 期望无输出

# 4) 渲染（1200 CSS px 宽，dpr 2，切片自 y=0 顺序拼接）
node shoot.js "file://$PWD/index.html" render
python3 stitch.py    # 断言位图高 == 页面 CSS 高 × 2

# 5) 指纹
shasum -a 256 index.html render/full@2x.png render/full@2x.gray.png \
  render/thumb.png
```

## 证据如何再来一遍（策略说明）

`prep_data.py` 按交付纪律只运行一次；如需**独立复核**证据（不覆盖本
目录），在别处重跑同款命令即可，例如：

```bash
V=~/projects/plot/vizir/target/release/vizir
S=~/projects/plot/vizir/examples/chart/service-health.viz.yaml

$V explain $S --node latency-risk/point/gateway      # Origin 六字段
$V render $S --format svg --output /tmp/a.svg --manifest /tmp/a.json
python3 -c "import json;print(len(json.load(open('/tmp/a.json'))['capability_report']['decisions']))"  # 174
$V capabilities svg                                   # unsupported_policy:"error"
env PATH=/nonexistent $V render $S --format png --output /tmp/x.png  # VIZ-CAP-0001, exit 1, 无残留
$V lower $S -o /tmp/s1.json && $V lower $S -o /tmp/s2.json && cmp /tmp/s1.json /tmp/s2.json
```

全部命令与输出样本冻结于 `data/`，指纹与门禁总表见 `VERIFICATION.md`。本块
6 条命令已于二轮修复后逐字实跑：explain 逐字段、manifest 174 条 decision 及
首条样本、capabilities 全量 JSON、VIZ-CAP-0001 exit 1 且零残留文件、双跑 lower
byte 级一致——全部与冻结数据逐项相符（详见 VERIFICATION.md §7/R8）。

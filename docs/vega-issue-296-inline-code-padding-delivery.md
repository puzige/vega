# Issue #296 · 行内代码留白交付

## Freeze

- UTC 2026-10-13T06:52:53Z / local 14:52:53+08:00；基线 cefb2f1。
- 契约：[padding spec](vega-issue-296-inline-code-padding.md)。macOS15.8.1 arm64，rustc/cargo1.98.0，git2.55.0。
- 5个Rust文件的源码manifest SHA256：`7a4499a3834598fd521dd5ac1f848495864282f447411572a36e25f998d2e09f`。
- 含code行使用GPUI字形断点与源范围片段，真实预留每侧4px；普通无code路径不变。多击选择保留原词/逻辑行语义，源文和剪贴板不插入装饰。

## Results

实际命令（均为进程内GPUI，非原生手测）：

```text
A cargo nextest run -p vega_ui -E 'test(issue296_padding_t01)'
B cargo nextest run -p vega_ui -E 'test(issue147_) | test(issue296_inline_) | test(issue59_) | test(issue119_) | test(width_remeasure_preserves_anchor_identity)'
C cargo nextest run -p vega_ui -E 'test(issue296_inline_t01) | test(issue296_inline_t02)'
D cargo nextest run -p vega_ui -E 'test(issue296_padding_t02) | test(issue296_padding_t03)'
E cargo nextest run -p vega_ui -E 'test(issue296_padding_) | test(issue296_inline_t02) | test(issue147_inline_)'
F cargo nextest run -p vega_ui -E 'test(issue296_padding_t03)'
```

| 日志 | 命令/exit | 原始footer |
|---|---|---|
| red | A/100 | Summary [   0.065s] 1 test run: 0 passed, 1 failed, 731 skipped |
| green-first | A/0 | Summary [   0.198s] 1 test run: 1 passed, 731 skipped |
| regression-first | B/100 | Summary [   1.354s] 26 tests run: 22 passed, 4 failed, 706 skipped |
| geometry-second | C/0 | Summary [   0.154s] 4 tests run: 4 passed, 728 skipped |
| review-red | D/100 | Summary [   0.075s] 3 tests run: 1 passed, 2 failed, 732 skipped |
| review-green | E/0 | Summary [   0.152s] 12 tests run: 12 passed, 723 skipped |
| generation-final | F/0 | Summary [   0.053s] 2 tests run: 2 passed, 733 skipped |

日志SHA256按上表顺序：

```text
966ba0dd201b37f678a1ce27dedf25c8cac96af9aa15b69938f9dc57ab121a27
57301caf8d0d816e38b4ec4244023d153507d0b666262e0e4599e0453a8856df
ac9b9a8abc3c38398772d13e5a03a322956ca6ec268ca028e4d6b9550c07d2c7
9648c67babd2e9b8e7b4c6f6b36626dd7a1f36ebc1cc872dc64ad16863563e59
6d3c64ffa1a05e1c1e7ba294b0b8236c4defb2fb572e4b4df0ea711c68895dfd
98fc7c444185e1a4ce521176197b985f5098f618efc2559059ed94d68baa4dc1
1b1957630f6bdd82e835974a5a197c21a9c84117c9e2d63d9ba8c41441befa1d
```

第一红测复现真实缺少留白。B中4失败为旧单TextLayout观察方式和被spec替代的窄片段几何；C迁移观察方式与明确数值后通过，未削弱源文/复制断言。D复现双/三击被限制在片段；E修复后通过，F补充generation清理后通过。选择绘制顺序、跨片段精确复制、浅深主题、中心/右对齐、CJK/emoji/混合文字与临界宽度均有定向证据。只读审查无剩余阻塞。

`cargo fmt -p vega_ui -p vega_theme --check` 与 `git diff --check` exit0。未跑本地全量或Clippy；既存block v0.1.6 future-incompatibility提示保留。云端门禁待PR执行，不将本地定向测试当全量通过。

## Residuals

- M01原生手测NOT RUN；合并后In review等待用户，无应用安装/重启。
- 无数据迁移，无新依赖或公开API；回滚本轮共享布局即可。
- 不改变既有GPUI极窄容器无法容纳单字形的行为；shape失败沿原空布局和日志语义。测试为进程内证据，不代表真实字体平台全组合。

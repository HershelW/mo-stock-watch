# 发布和迁移

## v0.2.3 北京时间与实时盈亏

程序启动后从 HTTPS 行情服务的服务器时间校时（腾讯，失败时尝试东方财富），每30分钟重新校准。显示时间、开收盘判断、行情日期、今日盈亏、交易默认日期统一采用北京时间 UTC+8，与 Windows 时区无关。网络响应缺少时间、带有非零缓存 Age 或耗时过长时拒绝校时。

校准后使用单调计时持续走时，修改系统时间不影响当前程序；断网沿用本次已校准时间，并在悬停说明中标记。首次启动尚未校准时显示“待校时”，暂停自动行情、今日盈亏和交易录入，每分钟重试。程序不修改 Windows 时区或系统时钟。旧存档的带偏移时间戳和交易日期保留原样，读取时按北京时间判断对应日期；首次校时前的离线备份/保存时间仅以 UTC+8 系统时钟兜底，不用于实时交易判断。

运行 `mo-stock-watch.exe --check-clock` 输出北京时间与校时来源，不读取或修改持仓。私有持仓及账本无需迁移。

修复有券商截图时今日盈亏被冻结的问题：以当天匹配的截图为基准，加上最新持仓市值变化，普通与极简窗口的金额及百分比同步刷新。行情缺失或早于截图时明确等待行情；持仓/现金更新后旧基准失效，按新持仓重新计算。股数、成本和现金不会随行情被修改，未知成交仍标记待核。

## v0.2.1 升级

先关闭运行中的程序并备份 `%APPDATA%\mo-stock-watch`。构建后运行 `run-release.bat`，固定入口是 `dist\mo-stock-watch.exe`，不再由日常启动器选择 debug 旧版。

旧存档可直接读取，但未知历史收益显示“待核”。已有旧私有账本时执行一次 `scripts/portfolio.ps1 -Command migrate`；之后使用 `update` 或 `rebuild`，不要再次扫描旧快照重建交易。迁移不更改当前现金、股数和成本。完整流程见 [PORTFOLIO_OPS.md](PORTFOLIO_OPS.md)。

回滚必须关闭程序并从同一备份恢复 `portfolio.json`、`trade_ledger.json` 和 `ledger_history.json`；不要混用不同版本的数据。迁移前的 xlsx 只作为历史文件保留，最新表格由 `export` 按需生成 CSV。

## 本地发布构建

```powershell
cargo build --release
```

构建产物：

```text
target\release\mo-stock-watch.exe
```

也可以双击：

```text
run-release.bat
```

## 发布包建议

如果只给自己使用，通常复制以下文件即可：

```text
target\release\mo-stock-watch.exe
README.md
docs\
```

如果要在另一台 Windows 电脑运行，需要确保目标电脑具备运行环境。一般 Rust 程序会静态链接大部分依赖，但 Windows 系统组件、字体、网络和证书环境仍依赖本机。

## 数据迁移

复制：

```text
%APPDATA%\mo-stock-watch\
```

该目录包含：

- `portfolio.json`
- `settings.json`
- `credentials.dat`（仅能由原 Windows 用户解密）
- `quote_cache.json`
- `market_holidays.json`
- `backups\`
- 最近一次剪贴板 OCR 图片缓存

换电脑时建议复制 `portfolio.json`、`settings.json` 和 `backups\`，然后在新电脑重新填写 API Key。

## GitHub 首次上传

推荐创建私有仓库：

```powershell
gh repo create mo-stock-watch --private --source . --remote origin --push
```

后续普通推送：

```powershell
git push
```

## 清理大文件

上传前可以确认仓库状态：

```powershell
git status -sb
git ls-files
```

如果目录很大，通常是 `target/`。它不会被 Git 上传，可以按需清理：

```powershell
cargo clean
```

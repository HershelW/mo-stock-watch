# 发布和迁移

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

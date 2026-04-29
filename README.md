# Mo Stock Watch

Mo Stock Watch 是一个轻量的 Windows 桌面 A 股持仓监控小窗，用 Rust + egui/eframe 编写。它面向个人使用：本地维护持仓，自动刷新东方财富行情，计算市值、持仓盈亏和今日浮盈，并支持从券商截图导入持仓草稿。

## 功能

- 本地维护 A 股持仓：代码、名称、数量、成本价。
- 多行情源刷新现价、昨收、涨跌幅：优先东方财富，失败后自动 fallback 到腾讯、新浪。
- 计算单票持仓盈亏、今日浮盈、总市值、总持仓盈亏、总今日浮盈。
- 非交易时段自动停止请求行情，工作日仅在 `09:20-11:30`、`13:00-15:00` 刷新。
- 支持窗口置顶、刷新间隔、字号调整、编辑/保存持仓。
- 支持本地 Tesseract OCR 导入截图草稿。
- 支持 OpenAI 兼容接口的 AI OCR，优先 Responses API，失败后尝试 Chat Completions。
- 持仓和 AI 配置保存在系统数据目录，源码目录可移动。

## 快速运行

需要先安装：

- Rustup / Rust stable
- Visual Studio Build Tools 2022，并勾选 `Desktop development with C++`

开发运行：

```powershell
cargo run
```

Release 运行：

```powershell
cargo run --release
```

也可以双击项目根目录的脚本：

- `start-mo-stock-watch.bat`：优先启动已有 debug 可执行文件；没有则自动 `cargo run`。
- `run-release.bat`：构建并启动 release 版本。

如果出现 `link.exe not found` 或 `kernel32.lib not found`，通常是 MSVC Build Tools 或 Windows SDK 没装完整。

## 数据位置

应用数据默认保存在：

- `%APPDATA%\mo-stock-watch\portfolio.json`
- `%APPDATA%\mo-stock-watch\settings.json`
- `%APPDATA%\mo-stock-watch\clipboard_ocr.png`

这些文件不在仓库里。移动源码目录不会丢持仓；换电脑时需要迁移上述数据文件。

## OCR

OCR 是可选能力，不会自动覆盖已有持仓，只会把识别结果追加为可编辑草稿，保存前需要人工核对。

本地 OCR 需要安装 Tesseract，并确保 `tesseract.exe` 在 PATH 中，或安装在常见目录：

- `C:\Program Files\Tesseract-OCR\tesseract.exe`
- `C:\Program Files (x86)\Tesseract-OCR\tesseract.exe`

AI OCR 在应用内填写 API Key、Base URL 和模型后使用。更多说明见 [docs/OCR.md](docs/OCR.md)。

## 开发

常用检查：

```powershell
cargo check
cargo test
```

`target/` 是 Rust 编译缓存和构建产物，可能达到数 GB，已被 `.gitignore` 排除，不会上传 GitHub。需要清理时运行：

```powershell
cargo clean
```

更多开发说明见 [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md)。

## 仓库内容

```text
src/
  app.rs        主窗口状态、UI、交互和后台任务轮询
  ai.rs         OpenAI 兼容接口 OCR、模型列表、模型测试
  config.rs     本地配置和持仓文件读写
  main.rs       eframe 入口
  ocr.rs        Tesseract OCR 和文本解析
  portfolio.rs  持仓模型、市场推断、数据清洗
  quote.rs      东方财富行情获取和盈亏计算
docs/
  USAGE.md       使用说明
  OCR.md         OCR/AI OCR 配置说明
  DEVELOPMENT.md 开发和维护说明
  RELEASE.md     发布和迁移说明
```

## 注意

这是个人桌面辅助工具，不构成投资建议。行情数据来自第三方公开接口，可能延迟、失败或字段变化；交易决策请以券商和交易所数据为准。

## License

MIT License. See [LICENSE](LICENSE).

# OCR 和 AI OCR

## 设计原则

OCR 只导入草稿，不自动覆盖原持仓。识别结果可能误读代码、数量或成本价，保存前必须人工核对。

## 本地 OCR

本地 OCR 使用 Tesseract：

```powershell
tesseract image.png stdout -l chi_sim+eng --psm 6
```

应用会优先查找：

- `C:\Program Files\Tesseract-OCR\tesseract.exe`
- `C:\Program Files (x86)\Tesseract-OCR\tesseract.exe`

如果都没有，就调用 PATH 中的 `tesseract`。

本地 OCR 的解析逻辑在 `src/ocr.rs`，主要流程是：

1. 识别截图文字。
2. 在每行里寻找 6 位股票代码。
3. 从同一行数字里猜测数量和成本价。
4. 推断沪/深/北市场。
5. 追加为持仓草稿。

## AI OCR

AI OCR 通过 OpenAI 兼容接口实现，代码在 `src/ai.rs`。

应用内需要配置：

- API Key
- Base URL，例如 `https://api.openai.com/v1`
- OCR 模型
- 分析模型

AI OCR 会：

1. 读取截图。
2. 转成 base64 data URL。
3. 请求 `/responses`。
4. 如果失败，尝试 `/chat/completions`。
5. 要求模型返回结构化 JSON。
6. 只保留 6 位代码、数量大于 0、成本价大于 0 的结果。

## 模型选择

不要选择 Codex 模型或 embedding 模型。推荐使用支持视觉输入的通用模型，例如：

- `gpt-4o`
- `gpt-4.1`
- `gpt-5.x-mini`
- 支持视觉的 Gemini / Claude / Qwen-VL 兼容通道

应用内的 `推荐OCR模型` 会从模型列表里挑一个看起来适合视觉 OCR 的模型。

## 剪贴板和拖放

打开 OCR 面板后：

- `Ctrl+V`：从剪贴板读取图片并走 AI OCR。
- 拖放图片：走本地 OCR。
- 点击选择图片：走 AI OCR。
- `本地OCR`：手动选择图片并调用 Tesseract。

## 隐私提醒

AI OCR 会把截图发送到你配置的模型服务。截图里如果包含账号、资产、资金、客户号等敏感信息，建议先裁剪，只保留持仓表格区域。

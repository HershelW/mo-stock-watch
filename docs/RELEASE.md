# 发布和迁移

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
- 最近一次剪贴板 OCR 图片缓存

如果不想迁移 API Key，可以只复制 `portfolio.json`，然后在新电脑重新配置 AI 设置。

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

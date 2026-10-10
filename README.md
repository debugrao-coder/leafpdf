# LeafPDF · 轻阅

[![Windows CI](https://github.com/debugrao-coder/leafpdf/actions/workflows/ci.yml/badge.svg)](https://github.com/debugrao-coder/leafpdf/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

Windows 10/11 x64 原生 PDF 阅读器，使用 Rust、Win32/GDI 和 PDFium。支持单页、连续和双页阅读，文字搜索，以及本地阅读记录。

A native Windows PDF reader built with Rust, Win32/GDI and PDFium.

## 下载与使用

从 [Releases](https://github.com/debugrao-coder/leafpdf/releases) 下载 `LeafPDF-Windows-x64.zip`，完整解压后双击 `leafpdf.exe`。`pdfium.dll` 必须保留在同一目录；运行时无需安装 Rust。程序未做代码签名，可用发布页的 `SHA256SUMS.txt` 核对压缩包。

默认最大化打开。可以将 PDF 拖到窗口或 EXE 上，也可以按 Ctrl+O 选择文件。

| 操作 | 使用方式 |
| --- | --- |
| 翻页 | 左右键、PageUp/PageDown；Home/End 跳到首尾页；缩略图和页码框也可定位 |
| 阅读布局 | 单页、连续、双页；侧栏可收起 |
| 缩放与旋转 | 适合页、适合宽；加减或 Ctrl+滚轮缩放；旋转每次 90° |
| 移动页面 | 拖动页面平移，滚轮纵向滚动，Shift+滚轮横向滚动 |
| 搜索 | Ctrl+F 输入文字并回车；F3 或“下个”跳到下个匹配页 |
| 工具栏 | 鼠标移到窗口内容顶部 8 像素区域时显示；F10 切换显隐，Esc 收起 |
| 主题 | 点击“暗黑 / 浅色”，程序自动记忆选择，PDF 页面保持原色 |

工具栏在鼠标离开后自动收起；输入页码或搜索文字时保持显示。输入框有焦点时，方向键保留正常输入行为。搜索按匹配页定位，暂不提供逐字高亮。

## 阅读记录与能力范围

最近 12 个文件的页码、布局、缩放和旋转保存在 `%LOCALAPPDATA%\LeafPDF\settings.json`，其中包含本地文件路径，未加密。PDF 内容不会复制到记录文件，也不会上传。可通过 `LEAFPDF_DATA_DIR` 指定记录目录。程序不注册开机启动或默认 PDF 文件关联。

支持文字型、扫描和图片型 PDF 显示；没有文字层的文档无法搜索。暂不提供 OCR、编辑、签名、表单填写、文本选择复制或加密文件的密码输入。

文件上限为 512 MiB、20,000 页。渲染缓存有数量和容量上限，特别高倍率下会限制内部栅格分辨率。

## 从源码构建

需要 Windows x64、Rust stable 的 MSVC 工具链、Visual Studio C++ Build Tools 和 Windows SDK；Python 3.11 或更高版本用于下载与打包。

```powershell
python scripts/download-pdfium.py
cargo build --release --locked
python scripts/package.py
```

便携目录生成在 `dist/LeafPDF/`，压缩包和校验文件生成在 `dist/`。打包脚本通过 Cargo metadata 定位依赖和构建产物，支持 `CARGO_HOME` 与 `CARGO_TARGET_DIR`。

PDFium 固定为 [157.0.8086.0 / chromium/8086](https://github.com/bblanchon/pdfium-binaries/releases/tag/chromium%2F8086) 的 Windows x64、no-V8/no-XFA 构建。下载地址和 SHA256 固定在 [scripts/pdfium.json](scripts/pdfium.json)。脚本先验哈希，再检查归档路径，保留上游全部许可证；打包前重新核对 DLL 和许可证是否与归档一致。`vendor/` 不进入源码仓库。

## 验证

```powershell
python -m pip install -r scripts/requirements-dev.txt
python scripts/generate-fixtures.py
python scripts/test-release.py
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
cargo build --release --locked
python scripts/package.py
python scripts/qa.py
```

测试 PDF 由脚本生成，只含合成文字和几何图形。`qa.py` 使用发布目录中的 EXE 和 DLL，在隐藏原生窗口中检查阅读布局、旋转、缩放与搜索，输出 `test-output/native/` 下的 PNG 和 `result.json`；阅读记录写入独立测试目录。

图标可通过 `python scripts/generate-icon.py` 从几何图形重新生成。修改图标后需要重新构建再打包。

## 许可证

LeafPDF 源码和脚本生成的项目图标采用 [MIT](LICENSE) 许可证。PDFium、其内含组件和 Rust 依赖保留各自许可证；下载到的 `vendor/LICENSE` 属于 PDFium 二进制构建项目，PDFium 本体声明位于 `vendor/licenses/pdfium.txt`。

发布包包含根 `LICENSE`、[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)、PDFium 的完整声明、每个 Cargo 依赖的许可证，以及实际 Rust 编译器附带的标准库版权声明和许可证。`build-info.json` 记录运行库、编译器版本和文件哈希，包内 `SHA256SUMS.txt` 可逐文件核对。重新分发时请一并保留。

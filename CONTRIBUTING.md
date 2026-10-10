# 参与开发

欢迎通过 GitHub Issues 报告问题，或提交 Pull Request。描述问题时请注明 Windows 版本、显示缩放比例、复现步骤和预期结果。请用可公开的合成 PDF 复现，避免上传含个人或工作资料的文档。

构建环境与命令见 [README](README.md)。改动应保持原生 Windows 实现，在 Windows 上通过格式检查、Clippy、测试和 release 构建。阅读状态和 PDF 渲染问题可使用仓内脚本生成测试文档，再以隔离数据目录运行 QA。

提交内容遵循本项目的 [MIT 许可证](LICENSE)。PDFium 和其他第三方组件保留各自许可证；更新 PDFium 时应同时更新固定版本、校验值和发布包中的声明。

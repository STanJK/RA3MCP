# 下载与参考源码

版本固定于 2026-10-17。先下载以下两个包；下载后保留原始 ZIP，方便核对版本。

| 文件 | 链接 | 用途 |
| --- | --- | --- |
| MinHook v1.3.4 源码 ZIP | https://github.com/TsudaKageyu/minhook/archive/refs/tags/v1.3.4.zip | **编译必需**。解压后的源码放到 `third_party/minhook`，或用 `-MinHookDir` 指定。 |
| Ultimate ASI Loader v9.7.4 Win32 NoPDB | https://github.com/ThirteenAG/Ultimate-ASI-Loader/releases/download/v9.7.4/Ultimate-ASI-Loader-NoPDB.zip | **加载必需**。取 loader DLL 重命名为 `d3d9.dll`；不要下 `_x64`。 |

UAL Win32 NoPDB ZIP 发布资产的 SHA-256：

```text
14b3a1ad018899571ac9aa01482977f3c6d49e6cba99f552d01c5acacd1315e1
```

如需 UAL 自身的调试符号，可改下完整 Win32 包（两种二进制包二选一）：

- https://github.com/ThirteenAG/Ultimate-ASI-Loader/releases/download/v9.7.4/Ultimate-ASI-Loader.zip
- 该发布资产 SHA-256：`952cebfc30d525afc2bdbaca954329d405ded3aa688a83027354dae14dfd5c5f`。

在 Windows 上可用 `Get-FileHash .\Ultimate-ASI-Loader-NoPDB.zip -Algorithm SHA256` 核对。

2026-10-17 收到的两个 ZIP 已进行静态校验：

| 内容 | 实测 SHA-256 / 架构 |
| --- | --- |
| `minhook-1.3.4.zip` | `172708123daa0c98d20d3a980b16a50be14af243dc95dee6f79c24193ad010e4`，编译所需源码和许可证齐全。 |
| `Ultimate-ASI-Loader-NoPDB.zip` | `14b3a1ad018899571ac9aa01482977f3c6d49e6cba99f552d01c5acacd1315e1`，与上述发布资产摘要一致。 |
| 包内 `dinput8.dll` | `ec2f4824eca58dd40f425756a4a7cec77b8e381f21d11d7c846ec4b339b617ab`，PE Machine `0x014c`（x86）。 |

把这两个原始 ZIP 放进 `downloads/` 后，本机执行 `scripts/Prepare-Dependencies.ps1` 即可校验并准备依赖。Loader 会另存为 `downloads/ual-win32/d3d9.dll`，原始 ZIP 保留。MinHook 摘要是本次收到文件的实测值，不宣称为上游签名或上游发布的摘要。此步骤不验证游戏实际加载效果。

## 需要参考的源代码

| 源码 | 固定版本下载 | 重点文件 |
| --- | --- | --- |
| MinHook v1.3.4 | https://github.com/TsudaKageyu/minhook/archive/refs/tags/v1.3.4.zip | `include/MinHook.h`、`src/hook.c`、`src/trampoline.c`、`src/hde/hde32.c`、`LICENSE.txt`。构建直接使用此包。 |
| Ultimate ASI Loader v9.7.4 | https://github.com/ThirteenAG/Ultimate-ASI-Loader/archive/refs/tags/v9.7.4.zip | `source/dllmain.cpp`：延迟加载、`InitializeASI` 调用与 `d3d9.dll` 转发。仅参考，不参与本项目编译。 |
| RA3MCP 本项目 | https://github.com/STanJK/RA3MCP | `native/src/bridge.cpp`、`controller/src/lib.rs` 和 `controller/src/main.rs`。 |

UAL 源码已从用户提供的加密外层 ZIP 解出；内层 `Ultimate-ASI-Loader-9.7.4.zip` 的实测 SHA-256 为 `d3a1844984abe8b05eb64f7993a654ece0e97f27bbe4c096e17c700d017b834f`，CRC 检查正常。静态检查 `source/dllmain.cpp` 确认了 `d3d9.dll` 转发分支、通过 `GetProcAddress` 调用无参数 `InitializeASI` 的路径，以及 `DontLoadFromDllMain` 延迟加载配置。源码仅供参考，不参与 RA3MCP 编译；实际进入游戏的加载效果仍待本机验证。

已静态检查同一 Win32 loader 的 PE 导出表，确认存在 `Direct3DCreate9` 与 `Direct3DCreate9Ex`；`source/dllmain.cpp` 的 D3D9 分支默认转发系统库。当前部署使用 D3D9，移出已有图形代理，不要求与 ReShade 共存。实际进入游戏的加载效果仍待本机验证。

不需要下载 ReShade 源码、整个 DirectX SDK 或 MinHook 的预编译 lib/bin。首版不 hook D3D9，不调用 D3DX；当前所需 Windows API 来自 Windows SDK。

## 本机缺工具时再装

| 工具 | 官方链接 | 安装要求 |
| --- | --- | --- |
| Visual Studio Build Tools 2022 | https://aka.ms/vs/17/release/vs_buildtools.exe | 选择“使用 C++ 的桌面开发”、MSVC v143 x64/x86 和一个 Windows 10/11 SDK。已有完整 VS2022 对应组件时无需重复安装。 |
| CMake | https://cmake.org/download/ | 稳定版 Windows x64 Installer，加入 PATH；至少 3.20。已有 VS 随带并可在 PATH 使用的 CMake 时无需重复安装。 |
| Rustup Windows x64 MSVC | https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe | 安装 stable MSVC 工具链及 Cargo；控制器可以是 x64，ASI 仍由 MSVC 编译为 x86。 |

构建需要的源码 ZIP 和工具安装程序是不同事项；无需把已安装工具的整个目录传回。优先发 MinHook 源码 ZIP、UAL Win32 二进制 ZIP、UAL 固定版本源码 ZIP。

官方版本页：

- MinHook：https://github.com/TsudaKageyu/minhook/releases/tag/v1.3.4
- ASI Loader：https://github.com/ThirteenAG/Ultimate-ASI-Loader/releases/tag/v9.7.4
- Rust 安装说明：https://rust-lang.org/tools/install/

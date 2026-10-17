# RA3MCP

红警 3 鼠标控制桥接首版：`RA3InputBridge.asi` 在游戏进程内转发输入，外部 Rust `ra3ctl.exe` 通过命名管道控制移动、点击、拖动与滚轮。项目当前是输入桥接基础，尚未提供 MCP server。

**状态：本机 MSVC Win32 Release 构建、1 项 CTest 和 3 项 Rust 测试已通过；游戏内加载和输入效果待验证。** 后续编译与测试仅通过用户本机 Toolhub 运行，远端只进行静态检查。尚不能把这份源码视为已验证的游戏插件。

## 目标版本

- 32 位 `ra3_1.12.game`，IDA image base `0x400000`。
- 文件 SHA-256：`dc54c54aa40d48bf2605cd5616f01e19fb1b3859b9dbe03dfa6dc28e6921e4b0`。
- 模块加载后先核对架构、映像范围、文件哈希；不匹配时记录日志并拒绝安装 hook。
- 实际加载目标是 `Data/ra3_1.12.game` 所在进程，不是外层启动器。

地址、对象关系和调用约定见 [逆向记录](docs/REVERSE_ENGINEERING.md)。

## 本机准备与构建

依赖下载链接、版本与用途见 [下载清单](docs/DOWNLOADS.md)。需要 MSVC x86 C++ 工具链、Windows SDK、CMake，以及 Windows MSVC Rust 工具链。Rust 控制器可编译为 x64；ASI 必须为 x86。

将收到的两个原始 ZIP 放到仓库 `downloads/`，先在本机执行离线准备脚本：

```powershell
.\scripts\Prepare-Dependencies.ps1
```

脚本核对固定 SHA-256，把 MinHook 源码放到 `third_party/minhook`，并从 Loader 包的 `dinput8.dll` 准备 `downloads/ual-win32/d3d9.dll`。它不编译、不下载，也不改游戏目录；遇到内容不同的现有文件会拒绝覆盖。ZIP 在其他位置时可传 `-MinHookZip` 和 `-LoaderZip`。

也可手工将 MinHook v1.3.4 源码解压、重命名为以下目录；不要求下载 MinHook 预编译库：

```text
RA3MCP/
  third_party/minhook/include/MinHook.h
  third_party/minhook/src/hook.c
  third_party/minhook/LICENSE.txt
```

在本机仓库根目录用 PowerShell 执行：

```powershell
.\scripts\Build.ps1
.\scripts\Test.ps1
.\scripts\Stage.ps1
```

脚本默认使用 Visual Studio 2022 CMake generator，可通过 `Build.ps1 -Generator 'Visual Studio 18 2026'` 适配已安装且受当前 CMake 支持的版本。MinHook 也可由 `-MinHookDir 'D:\deps\minhook-1.3.4'` 指定；打包时传同一路径。

Rust 无第三方 crate，脚本使用 `--locked --offline`。首次安装工具链本身仍需要 Rustup 下载。构建脚本不下载依赖，也不自动修改游戏目录。

## ASI 加载（D3D9）

1. 退出游戏和启动器。Steam 启动选项使用 `-runver 1.12 -win -xres 1920 -yres 1080`，确认实际进程为 `Data/ra3_1.12.game`；桥接仍会核对文件哈希。
2. 使用用户已确认游戏会加载 `d3d9.dll` 的目录作为「加载目录」。以下配置与 `scripts` 均放在这个目录旁边；不要把 loader 与插件分到两个不同的加载目录。
3. 将先前为本项目部署的 UAL `winmm.dll` 移到游戏目录外保存。若加载目录已有 `d3d9.dll` 或 `d3d9Hooked.dll`，也先移出保存；本方案不与 ReShade 共存。
4. 把 `downloads/ual-win32/d3d9.dll` 复制到加载目录。它是已校验的 UAL v9.7.4 Win32 loader，包内原名为 `dinput8.dll`；不需要重新下载或编译。
5. **手动创建**加载目录下的 `scripts` 文件夹，将 `dist/Release/scripts/RA3InputBridge.asi` 放进去。游戏和 UAL 不会为部署自动创建这个文件夹。
6. 将 `config/global.ini.example` 的配置合并到加载目录的 `global.ini`；没有该文件时复制并去掉 `.example` 后缀。`DontLoadFromDllMain=1` 使 UAL 在其延迟加载路径调用 `InitializeASI`。
7. 启动游戏，查看加载目录的 `scripts/RA3InputBridge.log`。成功时有 `Ready: \\.\pipe\RA3MCP.<PID>`。没有日志时还需检查进程模块、命名管道和日志目录写权限，不能仅凭日志缺失认定 DLL 未加载。

例如加载目录确定为游戏 `Data` 时，部署布局为：

| 文件 | 游戏内位置 |
| --- | --- |
| UAL loader | `Data/d3d9.dll` |
| UAL 配置 | `Data/global.ini` |
| 桥接插件 | `Data/scripts/RA3InputBridge.asi` |
| 运行时日志 | `Data/scripts/RA3InputBridge.log` |

D3D9 在本版只负责把 ASI 加载进游戏。桥接的鼠标输入仍通过游戏原生输入泵与队列转发。UAL 源码与 PE 导出表已静态确认支持 D3D9；这台机器上的完整加载链仍需游戏内实测。

ASI 通过 `InitializeASI` 启动工作线程，`DllMain` 不安装 hook。模块初始化后固定驻留至进程退出；**首版没有热卸载**，替换插件前退出游戏。卸载时移走新增加的 ASI/loader，按备份恢复已有文件。

## 点击和拖动

用日志里的游戏 PID 替换下面的 `12345`。坐标是游戏窗口客户区像素；不包含标题栏和窗口边框。

```powershell
.\dist\Release\ra3ctl.exe --pid 12345 ping
.\dist\Release\ra3ctl.exe --pid 12345 move 800 450
.\dist\Release\ra3ctl.exe --pid 12345 click left 800 450
.\dist\Release\ra3ctl.exe --pid 12345 click right 850 500 100
.\dist\Release\ra3ctl.exe --pid 12345 drag left 300 250 700 550 600 30
.\dist\Release\ra3ctl.exe --pid 12345 wheel 800 450 -1
.\dist\Release\ra3ctl.exe --pid 12345 release
```

拖动末尾的 `600 30` 表示期望 600ms、30 个移动采样。每次指令等待输入循环回执；低帧率会延长实际耗时。成功打印 `pumped sequence=... pump=... held=0`，只表示输入循环已处理指令，不能据此认定游戏选中了单位或接受了命令。

当前修改的是游戏内部鼠标输入，**不保证移动 Windows 系统光标**。第一轮验证请保持游戏在前台、不要同时操作实体鼠标；原生输入仍在运行，可能覆盖合成输入。后台控制、独占输入、按键修饰符及游戏逻辑命令不在本版承诺内。

同一游戏只接受一个管道连接。连接断开或指令超时后，会请求在下一次游戏输入循环释放本插件记录的按键；游戏输入线程停住时，释放也必须等待它恢复。超时的点击或拖动可能已执行一部分，不要自动重试。

## 验证与后续接入

本机验证步骤和 Toolhub 待配置命令见 [本地验证计划](docs/TOOLHUB.md)。[协议文档](docs/PROTOCOL.md) 可以直接用于后续 Rust MCP server 或其他控制器。

项目采用 Apache-2.0；静态链接的 MinHook 保留其 BSD 许可证，`Stage.ps1` 会一并复制。ASI Loader 单独下载并遵循其许可证。仓库不包含游戏二进制、游戏资源或第三方预编译 DLL。

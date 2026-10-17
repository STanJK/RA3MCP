# Windows 启动链跟踪

`ra3trace.exe` 是外部 Rust 诊断启动器。使用 `CreateProcessW(DEBUG_PROCESS)`、`WaitForDebugEventEx` 和 `ContinueDebugEvent` 跟踪其启动的进程及继承调试链的子进程，不注入 ASI，不修改目标内存。正常结束、到达时限或 Ctrl+C 后停止跟踪并尝试脱离仍存活的进程；不主动结束游戏。

## 本轮已知事实

2026-10-17 的桥接日志显示根目录 ASI 进入了 `runme.exe`（PID 37980）和 `RA3.exe`（PID 39656），还没有 `.game` 的初始化记录。因此，根目录部署目前只证实了启动器加载；不能由此认定游戏子进程也加载了代理。

用户确认 `runme.exe` 是 Steam 的代理入口；本轮手动跟踪从游戏根目录的 `RA3.exe` 开始，传入 `-win -xres 1920 -yres 1080 -runver 1.12`。已提供的启动命令显示 `Data/RA3_1.12.game` 收到窗口参数和 `-config "<游戏根目录>/RA3_English_1.12.SkuDef"`；实际父子关系和 DLL 路径仍由跟踪日志确认。

首轮跟踪记录了 `RA3.exe`（PID 27128）创建 `Data/ra3_1.13.game`（PID 38488）；根目录 WinMM 和 ASI 进入启动器，游戏加载的是系统 WinMM/D3D9。该轮跟踪器把普通参数也分别加上引号，并将 canonicalize 的 `\\?\` 路径直接传给启动器，生成的原始命令与用户能正常启动 1.12 的手动命令不同。当前已修正为普通 drive/UNC 路径，并只在参数为空或包含空格、制表符、双引号时转义。新增完整 RA3 原始命令回归测试，以及读取子进程 `GetCommandLineW` 的实际传参验证；修正后的游戏版本与加载路径仍待用户复测。

跟踪目标是逐个回答：实际创建了哪些进程、它们的父 PID 是谁、`.game` 从哪里启动、每个进程加载的 `winmm.dll` / `d3d9.dll` 是本地还是系统文件，以及 `RA3InputBridge.asi` 是否映射进 `.game`。

## 构建

使用现有 Toolhub `cargo-check`、`cargo-build`、`cargo-test` 即可。新二进制通过 Cargo 自动发现，无第三方依赖，不需要新增 Toolhub operation。

Release 产物：`C:\STanJK\Development\RA3MCP\target\release\ra3trace.exe`。`scripts/Stage.ps1` 会把它复制到 `dist/Release/ra3trace.exe`。

Rust 测试包含命令行转义、JSON 转义、Windows 调试结构 ABI，以及启动两层测试子进程并收集 Unicode 调试输出的冒烟测试。两个标记 ignored 的函数是冒烟测试调用的子进程 fixture，不代表跳过主要验证。若本机已有 `build/win32/Release/ra3_core_tests.exe`，同一冒烟测试还会验证 64 位跟踪器跟踪 Win32 程序。

## 手动跟踪游戏

先退出已有的游戏和启动器，保持 Steam 正常运行。在 PowerShell 执行：

```powershell
cd C:\STanJK\Development\RA3MCP

.\target\release\ra3trace.exe `
    --exe 'C:\Program Files (x86)\Steam\steamapps\common\Command and Conquer Red Alert 3\RA3.exe' `
    --seconds 90 `
    -- -win -xres 1920 -yres 1080 -runver 1.12
```

本轮 `--exe` 固定选择 `RA3.exe`。`--` 后面的参数传给它，工作目录设置为游戏根目录，由启动器选择 1.12 版本并传入 SkuDef。请使用修复后重新构建的跟踪器；`trace_start` 的命令应为 `"C:\Program Files (x86)\Steam\steamapps\common\Command and Conquer Red Alert 3\RA3.exe" -win -xres 1920 -yres 1080 -runver 1.12`，普通参数不再额外加引号。

默认 JSONL 保存到 `ra3trace.exe` 旁边的 `logs/ra3trace-<时间>-<跟踪器PID>.jsonl`，启动时会显示完整位置。也可通过 `--out 'D:\traces\ra3-startup.jsonl'` 指定一个尚不存在的文件，已有文件会拒绝覆盖。

等游戏进入主菜单，保留跟踪直到自动达到 90 秒，或在跟踪器控制台按 Ctrl+C。把生成的 JSONL 和同轮桥接日志一起提供。无需复制跟踪器到游戏目录，也无需更换当前代理。

## 记录内容

| 事件 | 主要字段 |
| --- | --- |
| `trace_start` | 根进程完整命令行、工作目录、跟踪时限 |
| `process_create` | PID、线程 ID、父 PID、可执行文件实际路径、映像基址 |
| `dll_load` / `dll_unload` | PID、加载文件路径和基址，卸载时记录基址 |
| `debug_string` | `OutputDebugString` 内容，包含桥接自己的诊断 |
| `exception` | 异常码、地址、first-chance 状态和继续处理方式 |
| `process_exit` | PID 和退出码 |
| `trace_stop` | 结束原因、计数、准备脱离的 PID |

每行带单调递增的相对毫秒时间。文件记录全部 DLL；控制台突出进程事件、代理、桥接和 Steam API。路径优先从系统提供的映像文件句柄解析，失败时从进程映像或已映射文件回退读取；完全无法取得时明确标记，不猜测文件路径。

## 使用边界

调试事件会短暂停住产生事件的进程，可能改变启动时序；这份跟踪用于定位加载链，不能当作未调试运行的性能测量。只消化每个进程初始的系统加载断点，其他异常交回应用处理。

`DEBUG_PROCESS` 跟踪继承调试链的后代；如果 Steam 已有进程另行代启游戏，或后代主动开启新的调试链，该进程可能不在当前事件流里。`all_debugged_processes_exited` 只代表已跟踪进程退出，不能单独证明系统中没有游戏进程。出现这个缺口时，再用系统范围的 ETW / Process Monitor 补充，不根据缺失事件直接下结论。

跟踪器调用 `DebugSetProcessKillOnExit(FALSE)` 并在退出路径使用 `DebugActiveProcessStop`，不主动终止被跟踪进程。若脱离失败，控制台会显示对应 PID 和系统错误。

## Windows API 依据

- [进程创建标志：DEBUG_PROCESS](https://learn.microsoft.com/en-us/windows/win32/procthread/process-creation-flags)
- [DEBUG_EVENT 结构](https://learn.microsoft.com/en-us/windows/win32/api/minwinbase/ns-minwinbase-debug_event)
- [WaitForDebugEvent 与句柄生命周期](https://learn.microsoft.com/en-us/windows/win32/api/debugapi/nf-debugapi-waitfordebugevent)
- [DebugActiveProcessStop](https://learn.microsoft.com/en-us/windows/win32/api/debugapi/nf-debugapi-debugactiveprocessstop)

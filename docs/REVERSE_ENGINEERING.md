# 红警 3 1.12 输入入口记录

以下来自用户授权连接的 IDA 数据库及函数反汇编检查，记录日期 2026-10-17。只适用于本文哈希匹配的 x86 PE，不可直接用于其他补丁、免安装修改版或其他架构。

本页集中记录已经落地的鼠标输入入口和加载诊断。Skirmish AI 的只读对象索引见 [AI Probe](AI_PROBE.md)；本轮进一步定位的 Personality Store、战略指令 setter、预算应用、状态选择、目标/战术生命周期和安全写入边界见 [AI 控制面研究](AI_CONTROL.md)。

| 项目 | 值 |
| --- | --- |
| 文件 | `ra3_1.12.game` |
| IDA image base | `0x400000` |
| SHA-256 | `dc54c54aa40d48bf2605cd5616f01e19fb1b3859b9dbe03dfa6dc28e6921e4b0` |
| MD5 | `97519b4ea1e9fb5ad0d53193d674b725` |
| 架构 | PE32 / x86 |

运行时地址一律为 `GetModuleHandleW(nullptr) + RVA`。桥接模块先核对主模块文件哈希，再安装 hook。哈希检测是版本约束，不代表运行时内存从未被其他插件修改。

## 首版直接使用的函数

所有表内函数返回 `int`，`self` 由 ECX 传入。这里的名称为分析用途，不是原程序公开符号。

| 分析名 | IDA VA | RVA | x86 签名/栈清理 |
| --- | --- | --- | --- |
| InputPump | `0x5416C0` | `0x1416C0` | `int __thiscall(void* inputManager)`，无显式参数 |
| MouseQueueMove | `0x8B03F0` | `0x4B03F0` | `int __thiscall(void* mouse, int x, int y, uint32_t time)`，`ret 0xC` |
| MouseQueueDown | `0x8B0450` | `0x4B0450` | `int __thiscall(void* mouse, int button, int x, int y, uint32_t time)`，`ret 0x10` |
| MouseQueueUp | `0x8B04C0` | `0x4B04C0` | 同 Down，`ret 0x10` |
| MouseQueueWheel | `0x8B0530` | `0x4B0530` | `int __thiscall(void* mouse, int x, int y, int ticks)`，`ret 0xC` |

MinHook detour 采用 `int __fastcall hookedPump(void* self, void* unusedEdx)`，匹配原函数 ECX 及栈布局；原 trampoline 保持 `__thiscall`。

## 对象与调用次序依据

- 输入循环保存传入的 ECX 为 `self`；鼠标对象位于 `self + 0x38`。因此 hook 使用当前实例中的嵌入对象，不缓存全局鼠标指针。
- 原循环在处理消息前后都调用队列消费函数 `0x8B2D60`，对象为 `self + 0x28`。
- 循环处理 `PeekMessage/TranslateMessage/DispatchMessage`，最后将 `self + 0x38` 传给鼠标位置发射函数 `0x8B3200`，sink 来自 `self + 0x24`。
- 首版在原输入循环前调用已有 enqueue 函数，让原循环消费消息；不直接调用框选、点击目标判定或玩法命令函数。
- 全局游戏 HWND 槽位：IDA VA `0xCD5C10`，RVA `0x8D5C10`，只用于客户区边界校验。
- 观测到的鼠标相关全局槽位还有 `0xCE009C`、`0xCEE414`；首版不依赖它们的初始化时机。

按钮编号为 0 左、1 右、2 中；Wheel 参数为刻度而非 Win32 `WHEEL_DELTA=120`。事件时间使用游戏也导入的 `timeGetTime()`。

## 加载选择

目标导入表包含 Direct3D9 的 `Direct3DCreate9`，也包含 WinMM 的 `timeBeginPeriod/timeEndPeriod/timeGetTime`。2026-10-17 本机尝试 WinMM 后未得到桥接加载日志；该现象还不足以判定底层原因。用户随后确认根目录的 D3D9 和 WinMM 代理均可加载，路径而非代理名称是本轮实测差异；当前脚本默认准备 UAL `d3d9.dll`，不要求与 ReShade 共存。

UAL v9.7.4 的 `source/dllmain.cpp` 确认包含 D3D9 转发、`InitializeASI` 导出查找/调用与 `DontLoadFromDllMain` 配置。提供的 Win32 loader PE 导出表确有 `Direct3DCreate9` 与 `Direct3DCreate9Ex`。早期根目录实验仅验证启动器加载，不能据此断言游戏也使用根目录。后续成功实验同时保留根目录和 Data 的副本；游戏 ASI 的实际位置为 `Data/scripts/RA3InputBridge.asi`，日志为同目录的 `RA3InputBridge.log`。主模块哈希校验仍读取实际运行的 `Data/ra3_1.12.game`。

本机 MSVC Win32 Release 已成功链接 ASI，CTest 1/1、Rust 测试 3/3 通过。`bridge.def` 直接导出 `InitializeASI`，修复了原下划线别名触发的链接错误；PE 导出表拼写尚需独立检查。

## 宿主与校验日志

启动器和游戏是独立进程。桥接记录当前主模块路径与 ASI 路径，每行带 PID；宿主名为 `RA3.exe` 时明确跳过，不对启动器应用游戏 RVA。其他宿主继续执行原 PE32/x86、映像范围和固定 SHA-256 校验。校验日志分别报告映像问题、主文件读取 Win32 错误、BCrypt NTSTATUS，以及实际和预期 SHA-256。

父进程跳过不会把 ASI 转发到子进程。只有对应 `.game` PID 的宿主日志才能证明游戏初始化了桥接；只有对应 PID 的 Ready 日志才能证明管道已创建。

## 启动链证据更新

2026-10-17 新版日志实际记录：PID 37980 的宿主为根目录 `runme.exe`（映像大小 `0x35000`，未通过游戏 RVA 范围校验），PID 39656 的宿主为 `RA3.exe`（明确跳过）。该日志未出现 `.game` 的初始化。因此此前根目录实验目前只证实启动器加载有效，不能等同于游戏进程加载成功。

用户随后确认 `runme.exe` 是 Steam 代理；手动诊断入口为根目录 `RA3.exe -win -xres 1920 -yres 1080 -runver 1.12`，游戏命令行为 `Data/RA3_1.12.game -win -xres 1920 -yres 1080 -config "<游戏根目录>/RA3_English_1.12.SkuDef"`。本轮跟踪直接选择 `RA3.exe`，父子进程关系以跟踪事件为准。

新增 `ra3trace.exe` 使用 Windows `DEBUG_PROCESS` 事件跟踪实际启动链和 DLL 映射，详细用法见 [LAUNCH_TRACE.md](LAUNCH_TRACE.md)。它不注入 ASI；若游戏由调试链外的 Steam 进程代启，需要系统范围跟踪补足。

2026-10-17 20:27（Asia/Shanghai）用户把 WinMM、global.ini、scripts 复制到 Data，根目录副本同时存在。PID 39836 的宿主为 `Data/RA3_1.12.game`，ASI 为 `Data/scripts/RA3InputBridge.asi`；machine=0x014C、magic=0x010B、SizeOfImage=0x00950000，实际 SHA-256 与固定预期一致。随后输出 Verified 和 `Ready: \\.\pipe\RA3MCP.39836`，证明该轮游戏内初始化、Hook 安装和管道创建成功。

20:30 移出根目录文件后的跟踪：RA3.exe PID 36340 只加载系统 WinMM；游戏 PID 40564 加载 Data/winmm.dll 和系统 WinMM，但没有 ASI 映射。此前推荐“只保留 Data 即可”尚未成立，不能继续作为已验证结论。

UAL v9.7.4 的 Init 默认调用 HookIAT；拦截未成功时只 LoadOriginalLibrary，不会自动 LoadEverything。LoadPluginsAndRestoreIAT 还过滤调用来源及 .bind 区域。该机制与“有代理无插件”吻合，但尚未确定本机触发了哪条分支；成功与失败实验的调试方式也需控制。下一轮只在 Data/global.ini 测试 DontLoadFromDllMain=0、ForceEntryPointHook=0 的直接初始化模式；保留固定哈希约束，尚未固化为默认配置。

## 尚待本机验证

1. 区分调试器/普通启动与 UAL 延迟初始化的影响；验证 Data 单目录直接初始化模式。双目录游戏加载与 Ready 已通过。
2. 命名管道握手，以及原循环消费合成事件后的实际 UI 点击、地图移动命令与拖动框选效果。
3. 窗口缩放/分辨率变更、实体鼠标同时输入的行为。

静态分析给出了调用约定和可用注入层，但尚未证明实时输入语义完全符合预期。若内置鼠标位置在后续原生采样中被覆盖，需要本地记录和调整输入层，不能靠静态报告宣称完成。

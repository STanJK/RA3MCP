# 本机 Toolhub 编译与验证计划

用户约束：远端仅编辑代码和静态检查；Cargo、C++ 编译、单元测试、打包与游戏内测试均在用户本机进行。当前仅提供待接入命令，尚未修改 Toolhub 配置，也未宣称 Windows 构建通过。

下面把 `D:\src\RA3MCP` 替换为真实 checkout 根目录。Toolhub 应以明确的仓库工作目录调用固定脚本，不把请求文本拼接到任意 shell 命令。每次从 GitHub 更新代码后，记录当前 commit SHA 和命令退出码。

| 建议命令名 | 程序 | 参数（数组） | 工作目录 |
| --- | --- | --- | --- |
| ra3mcp_build | `powershell.exe` | `-NoProfile`, `-File`, `D:\src\RA3MCP\scripts\Build.ps1`, `-Configuration`, `Release` | `D:\src\RA3MCP` |
| ra3mcp_test | `powershell.exe` | `-NoProfile`, `-File`, `D:\src\RA3MCP\scripts\Test.ps1`, `-Configuration`, `Release` | 同上 |
| ra3mcp_stage | `powershell.exe` | `-NoProfile`, `-File`, `D:\src\RA3MCP\scripts\Stage.ps1`, `-Configuration`, `Release` | 同上 |

配置期再根据用户已有 PowerShell 执行策略决定如何运行本地可信脚本。MinHook 路径不在默认目录时，给 build 和 stage 同时追加 `-MinHookDir` 与源码根目录。三个命令均不应自动下载依赖或复制文件到游戏目录。

## 编译后检查

1. `ra3mcp_build`：MSVC Win32 编译 ASI；本机 Cargo 离线编译控制器。
2. `ra3mcp_test`：执行 C++ 协议固定向量/格式拒绝/有界队列 FIFO 测试，以及 Rust 协议回执/拖动插值测试。
3. 在 VS Developer PowerShell 中运行 `dumpbin /headers build\win32\Release\RA3InputBridge.asi`，确认 `14C machine (x86)`；运行 `dumpbin /exports` 确认 `InitializeASI` 未修饰导出。
4. `ra3mcp_stage`：输出 `dist/Release`，包含 MinHook 许可证；该目录不含 UAL，需独立下载。

## 游戏内验收（建议离线单人局）

每轮记录游戏文件哈希、插件 commit、PID、游戏分辨率和日志。先确认 ReShade、游戏及 ASI 能共同正常启动，再进入输入测试。

| 项目 | 操作 | 应检查的结果 |
| --- | --- | --- |
| 握手 | `ra3ctl --pid PID ping` | 返回 pump 计数且游戏无卡顿 |
| UI 点击 | 点击一个已知菜单按钮 | 按钮确实响应，不仅仅有 Pumped 回执 |
| 右键点击 | 在单人局对已选单位右键地面 | 实际移动/取消行为符合点击位置 |
| 拖动 | 地图上执行左键矩形拖动 | 框选范围与起止客户区像素一致 |
| 异常断开 | 长拖动中结束控制器进程 | 输入循环继续运行后，合成按键能释放 |
| 暂停/加载 | 在输入循环不推进时发指令 | 控制器收到超时，不自动重复点击；恢复后无遗留按键 |
| 分辨率 | 改变分辨率，再发越界坐标 | 越界操作被拒绝，游戏继续正常运行 |
| 版本拒绝 | 对非匹配版本仅检查日志 | 插件拒绝 hook；不可为通过测试而移除哈希约束 |

`release` 指令仅请求释放本插件记录的按键；若游戏线程停住，它同样需要等待线程恢复。MVP 固定驻留，不提供热卸载。首次需要调整代码时，退出游戏后再替换 ASI。

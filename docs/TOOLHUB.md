# 本机 ToolHub 编译与验证计划

用户约束：远端仅编辑代码和静态检查；Cargo、C++ 编译、单元测试、打包与游戏内测试均在用户本机进行。

RA3MCP 本机 checkout：

```text
C:\STanJK\Development\RA3MCP
```

ToolHub 不调用 `scripts/Build.ps1` / `Test.ps1` / `Stage.ps1` 作为编译入口，而是直接暴露底层固定工具链操作。远端只能选择已经定义好的 project + operation，不能提供 executable、argv、cwd 或 shell 文本。

## 依赖准备（本机一次性操作）

把 `minhook-1.3.4.zip` 和 `Ultimate-ASI-Loader-NoPDB.zip` 放入仓库 `downloads/`，在本机手动运行 `scripts/Prepare-Dependencies.ps1`。脚本校验固定 SHA-256，准备 `third_party/minhook` 与 `downloads/ual-win32/d3d9.dll`，不编译、不下载、不改游戏目录。ZIP 在其他位置时可传 `-MinHookZip` 和 `-LoaderZip`。

此脚本仅用于本机准备，不作为 ToolHub operation 暴露；后续编译仍使用下列固定工具链命令。

## ToolHub project

建议 Local Build 项目：

```yaml
RA3MCP:
  description: RA3MCP native bridge and Rust controller
  root: 'C:\STanJK\Development\RA3MCP'

  operations:
    git-status:
      description: Show RA3MCP git status
      program: git.exe
      args:
        - status
        - --short
        - --branch

    git-pull:
      description: Fast-forward RA3MCP from its configured upstream
      program: git.exe
      args:
        - pull
        - --ff-only

    git-sync-generated:
      description: Safely recover a locally dirty Cargo.lock when origin/main contains the canonical incoming lockfile
      builtin: git-sync-generated

    native-configure:
      description: Configure RA3InputBridge for Visual Studio 2022 Win32 with local MinHook
      program: 'C:\\Program Files\\Microsoft Visual Studio\\18\\Community\\Common7\\IDE\\CommonExtensions\\Microsoft\\CMake\\CMake\\bin\\cmake.exe'
      args:
        - -S
        - .
        - -B
        - build/win32
        - -G
        - Visual Studio 18 2026
        - -A
        - Win32
        - -DMINHOOK_DIR:PATH=C:/STanJK/Development/RA3MCP/third_party/minhook
        - -DRA3_BUILD_BRIDGE=ON

    native-build:
      description: Build RA3InputBridge and native tests in Release
      program: 'C:\\Program Files\\Microsoft Visual Studio\\18\\Community\\Common7\\IDE\\CommonExtensions\\Microsoft\\CMake\\CMake\\bin\\cmake.exe'
      args:
        - --build
        - build/win32
        - --config
        - Release

    native-test:
      description: Run native CTest suite in Release
      program: 'C:\\Program Files\\Microsoft Visual Studio\\18\\Community\\Common7\\IDE\\CommonExtensions\\Microsoft\\CMake\\CMake\\bin\\ctest.exe'
      args:
        - --test-dir
        - build/win32
        - -C
        - Release
        - --output-on-failure

    cargo-check:
      description: Check ra3ctl with locked offline dependency resolution
      program: cargo.exe
      args:
        - check
        - --locked
        - --offline
        - -p
        - ra3ctl

    cargo-build:
      description: Build ra3ctl Release with locked offline dependency resolution
      program: cargo.exe
      args:
        - build
        - --locked
        - --offline
        - -p
        - ra3ctl
        - --release

    cargo-test:
      description: Test ra3ctl with locked offline dependency resolution
      program: cargo.exe
      args:
        - test
        - --locked
        - --offline
        - -p
        - ra3ctl
```

## 构建栈

### Native / C++

`native-configure` 等价于直接执行：

```text
"C:\\Program Files\\Microsoft Visual Studio\\18\\Community\\Common7\\IDE\\CommonExtensions\\Microsoft\\CMake\\CMake\\bin\\cmake.exe" -S . -B build/win32 -G "Visual Studio 18 2026" -A Win32 ^
  -DMINHOOK_DIR:PATH=C:/STanJK/Development/RA3MCP/third_party/minhook ^
  -DRA3_BUILD_BRIDGE=ON
```

然后：

```text
cmake --build build/win32 --config Release
ctest --test-dir build/win32 -C Release --output-on-failure
```

CMake 的 Visual Studio generator 自己调用 MSBuild，因此 ToolHub 不需要通过 Developer PowerShell 包一层构建脚本。

MinHook 必须已经位于：

```text
C:\STanJK\Development\RA3MCP\third_party\minhook
```

并至少包含 `include/MinHook.h` 与 CMakeLists.txt 要求的源码文件。

### Rust / controller

ToolHub 直接使用：

```text
cargo check --locked --offline -p ra3ctl
cargo build --locked --offline -p ra3ctl --release
cargo test --locked --offline -p ra3ctl
```

`--locked` 保证验证过程不会隐式改写 `Cargo.lock`；`--offline` 保证本机验证不会下载依赖。若后续添加新依赖，需要先在本机准备好依赖缓存并由远端提交 canonical `Cargo.lock`。

## 推荐验证顺序

正常一轮：

```text
git-pull
native-configure   # CMake 配置变化、clean checkout 或首次构建时
native-build
native-test
cargo-check
cargo-build
cargo-test
git-status
```

如果 CMake 配置未变化，可以跳过 `native-configure`。

如果本地只有 `Cargo.lock` 被工具自动改脏，而且 `origin/main` 已经包含新的 canonical `Cargo.lock`，可以使用 ToolHub 的 fail-closed `git-sync-generated`，不要暴露通用 `git reset/restore`。

## 编译后检查

1. `native-build`：MSVC Win32 编译 `RA3InputBridge.asi` 与 native tests。
2. `native-test`：执行 C++ 协议固定向量、格式拒绝、有界队列 FIFO 等 CTest。
3. `cargo-build`：离线 Release 编译 `ra3ctl.exe`。
4. `cargo-test`：执行 Rust 协议回执、拖动插值等测试。
5. 后续如需要，可再给 ToolHub 增加固定的 `dumpbin` 验证 operation；暂不引入任意 shell 或 PowerShell wrapper。

## Staging

`scripts/Stage.ps1` 当前仅负责将已经存在的 ASI、`ra3ctl.exe`、`ra3trace.exe`、许可证、示例配置和 README 复制到 `dist/Release`。它暂时不作为 ToolHub 编译能力暴露。部署目录固定为包含 `RA3.exe` 的游戏根目录：loader 写入 `./d3d9.dll`，配置写入 `./global.ini`，ASI 放入 `./scripts/RA3InputBridge.asi`，运行日志为 `./scripts/RA3InputBridge.log`。`Data/ra3_1.12.game` 是被控制的主模块路径。

如果 staging 进入日常自动验证流程，再单独实现固定语义 operation；不要因此引入通用 PowerShell 执行能力。

## 启动链诊断

`controller/src/bin/ra3trace.rs` 由 Cargo 自动发现，现有 `cargo-check` / `cargo-build` / `cargo-test` 会覆盖跟踪器，无需新 operation。`cargo-test` 的调试事件冒烟测试只启动自己的测试子进程，及本机已有的 Win32 `ra3_core_tests.exe` fixture，不启动游戏。

游戏跟踪由用户手动执行，命令与日志说明见 [LAUNCH_TRACE.md](LAUNCH_TRACE.md)。日志覆盖继承调试链的进程与 DLL 事件；Steam 通过已有进程代启等脱离调试链的情况仍需补充系统范围跟踪。

## 游戏内验收（建议离线单人局）

当前根目录部署日志已确认 ASI 进入 `runme.exe` 和 `RA3.exe`；游戏子进程加载仍需用跟踪器确认。每轮记录游戏文件哈希、插件 commit、PID、游戏分辨率和根目录 `scripts/RA3InputBridge.log`。先确认 D3D9 loader、游戏及 ASI 能正常启动，再进入输入测试。当前不要求与 ReShade 共存。

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

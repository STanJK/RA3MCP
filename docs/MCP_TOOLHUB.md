# RA3MCP：Toolhub stdio MCP 接入规范

版本：ra3ctl 0.2.0。接口以 `controller/mcp-tools.json` 和运行时 `tools/list` 为准。本文是 RA3MCP 对外接入合同；Toolhub 的代码、配置和注册工作由其专门会话负责，本仓库不自动修改或注册 Toolhub。

本轮用户已验收原 CLI 的点击、拖动和游戏输入泵握手。加载方式保持已验证的根目录 + Data 双份；Data 单目录的延迟和直接初始化尝试均未成功，现阶段不再简化。MCP 复用同一套 24 字节桥接协议，不启动、安装、注入或退出游戏。

## 1. 进程启动与托管

| 字段 | 值 |
| --- | --- |
| 服务名建议 | `ra3` |
| executable | `C:\STanJK\Development\RA3MCP\target\release\ra3ctl.exe` |
| argv | `mcp --transport stdio`，必须作为三个独立参数传入 |
| cwd 建议 | `C:\STanJK\Development\RA3MCP`；程序也可从其他工作目录运行 |
| transport | `stdio` |
| 环境变量/额外运行时 | 无需专用变量，无 Python/Node/第三方 crate 依赖 |
| 游戏操作 PID | 每次工具调用显式提供；不使用启动时固定 PID |
| 请求超时建议 | 120000 ms；超时发送取消通知，禁止自动重放鼠标动作 |

由 Toolhub 的 stdio 后端启动并复用一个常驻子进程，保持 stdin 打开、持续读取 stdout。Local Build operations 继续只负责构建和测试。MCP 服务在游戏尚未运行时也能完成协议协商和工具发现。

常见 MCP 客户端配置如下；Toolhub 如使用不同配置结构，按上表映射字段，`cwd` 属于宿主配置而非 MCP 协议字段：

```json
{
  "mcpServers": {
    "ra3": {
      "command": "C:\\STanJK\\Development\\RA3MCP\\target\\release\\ra3ctl.exe",
      "args": [
        "mcp",
        "--transport",
        "stdio"
      ],
      "cwd": "C:\\STanJK\\Development\\RA3MCP"
    }
  }
}
```

对应文件：`config/mcp-client.example.json`。手动启动命令：

```powershell
.\target\release\ra3ctl.exe mcp --transport stdio
```

正常启动后等待 stdin 上的 MCP 请求；stdout 不打印欢迎文字或 CLI 的 pumped 文本。启动诊断写 stderr。原来的 `ra3ctl --pid <PID> ...` CLI 保留。

## 2. 传输与协议版本

- JSON-RPC 2.0，每行一个 UTF-8 JSON 对象，以 LF 结束；接受 CRLF。没有 Content-Length 头。
- 单帧（含换行）最多 1 MiB，JSON 最大嵌套 64 层，重复对象键和非法 UTF-8 被拒绝。
- 请求 ID 使用字符串或整数，正在处理的 ID 不可重复。通知不带 ID，也不会收到回复。
- 只声明固定的 `tools` 能力，`listChanged=false`；没有动态工具变更、resources、prompts 或 HTTP 监听。
- 支持 `2026-07-28`，以及握手版本 `2025-11-25`、`2025-06-18`。不接受 JSON-RPC batch 数组。

### 2.1 Toolhub 现有握手客户端

推荐直接使用现有 MCP SDK 的 stdio client。握手示例，每段各占一行：

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"MCP-Toolhub","version":"1.0.0"}}}
```

服务端返回所选 `protocolVersion`、`capabilities.tools`、`serverInfo` 和 `instructions`。客户端随后发送：

```json
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}
```

请求支持版本时原样选择；其他握手版本会被反提议为 2025-11-25。客户端必须确认能支持返回版本。

### 2.2 2026-07-28 客户端

此版本无需 initialize；每个请求在 `params._meta` 提供协议版本及 clientCapabilities，clientInfo 可选。可从 server/discover 开始：

```json
{"jsonrpc":"2.0","id":1,"method":"server/discover","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{},"io.modelcontextprotocol/clientInfo":{"name":"MCP-Toolhub","version":"1.0.0"}}}}
```

每个后续请求同样携带该 _meta，不能依赖上一条请求或当前连接继承版本、能力。结果添加 `resultType=complete` 和 `_meta.io.modelcontextprotocol/serverInfo`。旧版握手结果不添加这两个现代协议字段。

## 3. 工具映射

Toolhub 应通过 `tools/list` 动态读取 description、inputSchema、outputSchema、annotations，并代理 `tools/call`。工具名保持下表原名；聚合网关可按自身惯例增加后端前缀。

| 工具 | 参数 | 行为 |
| --- | --- | --- |
| `ra3_list_instances` | `{}` | 枚举 RA3MCP 命名管道，不连接游戏 |
| `ra3_ping` | `pid` | 惰性连接/复用管道，等待输入泵回执 |
| `ra3_move` | `pid, x, y` | 客户区坐标移动 |
| `ra3_click` | `pid, x, y, button?, hold_ms?` | 点击，默认 left / 80 ms |
| `ra3_drag` | `pid, x0, y0, x1, y1, button?, duration_ms?, steps?` | 拖动，默认 left / 400 ms / 20 步 |
| `ra3_wheel` | `pid, x, y, ticks` | 非零有符号滚轮刻度 |
| `ra3_release` | `pid` | 释放该桥接自己记录的鼠标按键 |
| `ra3_disconnect` | `pid` | 关闭缓存管道，释放独占连接 |

参数规则：PID 为 1..4294967295；坐标为 0..65535 的整数，桥接进一步检查实际客户区宽高；button 为 left/right/middle；hold_ms 和 duration_ms 为 1..60000；steps 为 1..1000；ticks 为 -100..-1 或 1..100。未知参数、null 及错误类型会在游戏 I/O 前被拒绝。没有额外的 sleep 或切前台等待；hold/duration 是动作本身的持续时间。

## 4. 调用顺序与结果

建议顺序：发现实例 → 对选定 PID ping → 点击/拖动等操作 → 需要交还 CLI 时 disconnect。发现返回的 connected 仅表示当前 MCP 进程有该 PID 的缓存连接，不能当作存活证明；管道存在也不能证明游戏输入泵正响应。

旧版握手会话中调用示例：

```json
{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"ra3_list_instances","arguments":{}}}
{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"ra3_click","arguments":{"pid":12345,"button":"left","x":960,"y":540,"hold_ms":100}}}
{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"ra3_drag","arguments":{"pid":12345,"x0":600,"y0":300,"x1":1200,"y1":700,"duration_ms":700,"steps":30}}}
```

12345 是示例 PID，必须替换成当前端点的 PID。现代客户端对相同请求补齐第 2.2 节的 _meta。成功的鼠标结果示例：

```json
{
  "jsonrpc": "2.0",
  "id": 4,
  "result": {
    "content": [
      {
        "type": "text",
        "text": "{\"ok\":true,\"pid\":12345,\"status\":\"pumped\",\"sequence\":4,\"pump\":104,\"held\":0,\"detail\":0}"
      }
    ],
    "structuredContent": {
      "ok": true,
      "pid": 12345,
      "status": "pumped",
      "sequence": 4,
      "pump": 104,
      "held": 0,
      "detail": 0
    },
    "isError": false
  }
}
```

structuredContent 为机器可读结果；content[0].text 为相同 JSON 的序列化，兼容文本客户端。status=pumped 表示原输入循环已返回；sequence 是当前缓存管道连接的包序号（点击/拖动会发送多个包），pump 是游戏泵计数，held 是桥接按键位图，detail 是原协议字段。它们不代表单位是否选中或命令是否被玩法接受，仍应由用户观察。

## 5. 并发、连接与失败处理

所有工具在一个工作线程串行执行，鼠标动作之间不交错。读取 stdin 的线程独立处理取消。最多排队 64 个 job，最多缓存 16 个显式 PID 的管道连接。每次操作仍需携带 PID；缓存仅用于复用传输，不代表默认游戏会话。

管道为独占连接：MCP 连接某 PID 后，其他 CLI/MCP 控制器暂时无法占用它。`ra3_disconnect` 只关闭本服务缓存的该 PID 管道，不关闭游戏或卸载 ASI；未缓存的 PID 返回 disconnected=false。游戏重启后重新发现 PID。

错误分两类：

- 协议错误：JSON-RPC error。解析 -32700、无效请求 -32600、未知方法 -32601、无效 RPC 参数/未知工具 -32602、内部/排队错误 -32603；现代版本不支持使用 -32022。
- 工具错误：result.isError=true，structuredContent.ok=false；error 包含 code、message、outcome、retryable=false，可带 os_error。invalid_arguments / connect_failed 等未发出操作时 outcome=not_sent；已进入传输的失败保守标为 unknown。

禁止自动重发失败、超时或取消后的鼠标动作。一次动作可能已经部分执行，下一条显式调用可以重新连接，但不能把它解释为安全重试。参数全部校验后才连接管道；连接阶段最长等待约 3 秒，每个包的读/写各有 5 秒 I/O 截止时间。

## 6. 取消与退出

超时/用户取消时发送通知，requestId 保留原 ID 的 JSON 类型：

```json
{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":5,"reason":"Toolhub request timeout"}}
```

收到取消后停止后续动作，取消并排空仍挂起的 overlapped I/O，再关闭受影响管道。被取消请求不再返回结果；取消与完成竞态时客户端应忽略迟到回复。桥接在断线后请求下一次游戏输入循环释放自己持有的按键；这不是同步释放回执，也不能回滚已发生的游戏操作。

关闭 stdin 是正常退出信号：服务取消排队/进行中的请求，关闭缓存管道，等待工作线程清理并退出。stdout/stderr 应持续被宿主读取。Toolhub 如重启进程，重新进行对应版本的协商，不能重放未确认完成的游戏动作。

## 7. 本机验证与验收

本轮停止 Toolhub 调用后，最终提交由用户在本机复验并构建 Release：

```powershell
cd C:/STanJK/Development/RA3MCP
git pull --ff-only
cargo check --locked --offline -p ra3ctl
cargo test --locked --offline -p ra3ctl
cargo build --release --locked --offline -p ra3ctl
```

已有 Toolhub cargo-check / cargo-build / cargo-test 也能覆盖这些命令，是否调用由负责 Toolhub 的会话决定，不需要新构建 operation 或额外依赖。测试启动真实 ra3ctl mcp 和专用模拟命名管道，覆盖握手/现代元数据、schema 参数校验、UTF-8/JSON、分片回执、缓存连接、点击拖动序列、取消长按、取消阻塞 I/O、EOF 退出、5 秒超时和不重放；不连接真实游戏。

Toolhub 接好后：先在没有游戏时检查协议协商和 8 个工具发现；用户按已验证布局加载游戏后，再发现 PID、ping、由用户确认点击/拖动，最后 disconnect 并确认 CLI 能重新占用管道。

## 8. 官方规范依据

- [2026 版本与握手兼容](https://modelcontextprotocol.io/specification/2026-07-28/basic/versioning)
- [2026 stdio 传输](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/stdio)
- [2026 请求元数据](https://modelcontextprotocol.io/specification/2026-07-28/basic)
- [server/discover](https://modelcontextprotocol.io/specification/2026-07-28/server/discover)
- [工具、结构化结果与错误](https://modelcontextprotocol.io/specification/2026-07-28/server/tools)
- [取消](https://modelcontextprotocol.io/specification/2026-07-28/basic/patterns/cancellation)
- [2025-11-25 生命周期](https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle)


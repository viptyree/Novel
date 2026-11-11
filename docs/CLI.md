# Novel CLI 与 SSH 使用指南

`novel` 使用与桌面应用相同的 Rust 核心、模型设置和小说文件。无需打开桌面窗口，输入 `novel` 即可进入中文菜单。Windows、macOS、Linux 提供独立 CLI；Android 可使用 SSH 客户端连接家里的 Windows 主机。

## 安装与启动

Windows 安装器包含 `novel.exe`，安装时默认勾选加入当前用户 PATH。安装后重新打开 PowerShell、CMD 或 SSH 连接，再运行 `novel --version`。如果旧 SSH 会话仍找不到命令，可用安装目录中的 `novel.exe` 完整路径启动。升级前请等待正在运行的模型任务完成。卸载不删除用户数据或小说项目，仅清理由本安装添加的 PATH 条目。

独立发行包：将 `novel.exe` 或 `novel` 放到 PATH 中的目录。Unix 上运行 `chmod +x novel`。从源码安装：

```sh
cargo install --locked --path crates/novel-cli
# 或只在源码中运行
cargo run --locked -p novel-cli -- --help
```

独立 CLI 的构建无需 Node、Tauri、GTK 或 WebView：

```sh
cargo build --locked --release -p novel-cli
```

编译输出在仓库根目录 `target/release/`。桌面开发继续使用 `cd app && npm run tauri dev`；Windows 桌面开发/打包的前置步骤会构建 CLI 并准备安装器 sidecar。

## SSH 连接家中电脑

先在家中 Windows 11 配好 OpenSSH Server，并以与桌面应用相同的 Windows 用户登录。CLI 使用主机上的模型地址，本地 API 的 `127.0.0.1` 指家里的主机。

```sh
ssh -t username@home-host novel
# 也可以先登录，再输入 novel
ssh username@home-host
novel
```

菜单支持方向键、数字选择后 Enter。**Esc 返回上一级，Home 返回主页**；手机或缺少 Home 键的终端可在菜单、阅读页使用 H，或选择菜单末尾的“返回上一级 / 返回主页”。主页按 Esc 不退出，退出程序请选主页的“退出”。返回会保留上一级的选中项和滚动位置；阅读正文、报告或执行操作后也会回到进入它的菜单。

顶部显示当前位置，窄屏优先显示当前页面。内置编辑器用 Ctrl+S / F2 保存，Ctrl+F 搜索，Ctrl+A / Ctrl+E 移到行首 / 行尾。搜索时先按 Esc 关闭搜索；编辑内容未保存时，Esc 或 Home 会提示“保存后离开 / 放弃修改 / 继续编辑”。保存失败会留在编辑器中，保留当前稿件。外部编辑器内的快捷键由该编辑器决定，退出后回到原操作菜单。

“生成 / 重新生成”的设置页包含“提示词预览（不调用模型）”“本章生成方案”“恢复自动组装”和“提交后台生成”。本章方案可选择跟随项目默认、通用方案或精简四段式，不修改项目默认。

预览菜单可查看完整提示词、分别编辑系统与章节提示词，或采用当前预览。编辑器 Ctrl+S / F2 保存即采用本章修改，不立即调用模型；无改动时也可通过“采用当前预览用于本章”确认。返回、Home、切换菜单后仍保留本章修改，退出程序后清除；两套项目模板不受影响。修改骨架、字数、方案、上下文或资料后，旧预览会被拦截，重新进入预览会组装最新内容；“恢复自动组装”清除本章采用内容及导入文件，保留其他生成设置。

独立预览入口共用同一章草稿。指定提示词覆盖文件时，预览显示覆盖内容并校验来源是否过期。预览和编辑均不调用模型、不覆盖正文。

生成设置在独立页面调整，idea 和选项在本次 TUI 会话中保留，回主页后重新进入仍可继续编辑；退出程序后不保留这份会话草稿。返回或回主页不取消已经提交的后台任务。

提交生成、一致性检查、记忆更新或章名任务后，菜单直接进入任务页，自动刷新显示项目、章号、阶段与耗时。成功后可直接阅读正文、编辑正文及查看对应报告；章名候选在“任务详情 / 错误”中查看。任务失败或中断时可查看完整错误和阶段日志。任务中心重新打开任务也使用此页面，结果操作始终针对提交时的项目。正文与报告支持中文换行、翻页滚动，在本次 CLI 会话中保留从任务页阅读的位置；退出 CLI 后阅读位置清除。返回菜单或断开 SSH 不取消任务。

`--plain` 使用相同的结果入口，通过“刷新状态”手动更新任务进度。当前展示业务阶段，正文在生成完成后可读，尚不支持边生成边显示正文。

终端对全屏支持较差时用 `novel --plain`：编号菜单使用 `0` 返回上一级、`H` 回主页。短文本输入使用 `.back` / `.home`，多行编辑使用 `.save` 保存、`.back` 返回、`.home` 回主页；离开前同样会提示处理未保存内容。首行输入 `.append` 可在原文后追加。


模型操作在独立 worker 中执行。标准命令默认等待完成，`--detach` 提交后返回任务 ID。关闭菜单、断开 SSH 或 Ctrl+C 停止等待均不会取消任务。重连后可进入任务中心，或执行：

```sh
novel job list
novel job show JOB_ID
novel job logs JOB_ID
novel job logs JOB_ID --follow
novel job wait JOB_ID
novel job cancel JOB_ID
```

取消请求在安全检查点生效：正在阻塞等待的模型请求可能需要等到响应或原有超时；保存结果期间不强行杀进程。主机关闭、重启或 worker 崩溃后任务标为 `interrupted`，不会自动重跑，尤其避免重复追加记忆。主机应保持运行；不提供模型现场恢复或持久队列。同一本书已有任务时新写入操作会返回“资源忙”。

Windows worker 使用 `DETACHED_PROCESS` 和 `CREATE_BREAKAWAY_FROM_JOB` 脱离 SSH 会话；如果系统策略阻止启动，会明确报错。程序不会悄悄降级为断连即终止的会话子进程。

## 数据与项目选择

与 GUI 共用以下目录：

| 主机系统 | 应用数据目录 |
| --- | --- |
| Windows | `%APPDATA%\com.yuxin.novel-pipeline` |
| macOS | `~/Library/Application Support/com.yuxin.novel-pipeline` |
| Linux | `$XDG_DATA_HOME/com.yuxin.novel-pipeline`，未设置时为 `~/.local/share/com.yuxin.novel-pipeline` |

目录包含 `.ui-settings.json`、`projects.json` 和任务状态 `jobs/`。首次从菜单启动可初始化默认项目 `novel-project/`；也可显式新建或打开项目。读取丢失的项目不会悄悄创建目录。

```powershell
novel project list
novel project current
novel project create "D:\小说\新书" --name "新书"
novel project open "D:\小说\已有项目"
novel project pin "D:\小说\已有项目"
novel project unpin "D:\小说\已有项目"
novel project relocate "D:\旧目录" "E:\新目录"
novel project forget "D:\不再显示的项目" --yes
```

`forget` 仅移除列表记录。`--project PATH` 对本次操作指定项目，不改全局当前项目：

```powershell
novel --project "E:\另一本书" chapter list
```

GUI 与已打开的 TUI 固定自己的项目，另一个终端执行 `project open` 不会改变编辑中的目标。后台任务也固定提交项目。可同时查看已保存内容；长任务期间修改同一项目会被阻止。保存时若内容已被另一入口修改，会报告冲突；CLI 编辑稿保留于应用数据目录的 `edit-recovery/`，GUI 编辑内容仍留在编辑框。

`--data-dir PATH` 是高级隔离选项，用于测试或单独配置；正常使用不要指定，否则不会读取默认 GUI 设置。共享文件上的锁是 Novel 各入口之间的协作保护，外部手工编辑器直接修改原文件不受锁约束。

## 写作流程

```powershell
novel chapter new ch001
novel chapter generate ch001 --idea-file idea.txt --target-words 4000 --overwrite
novel chapter show ch001
novel chapter edit ch001
novel chapter check ch001
novel memory update ch001
novel report show ch001 --kind memory
```

新建空章后生成需要 `--overwrite`，因为该章正文文件已存在；也可直接对尚不存在的章号执行 `generate`。生成会备份被覆盖的正文。检查与记忆更新是独立步骤，生成后不会自动更新长期记忆。

生成参数：

| 参数 | 作用 |
| --- | --- |
| `--idea TEXT` / `--idea-file FILE` / `--idea-stdin` | 三选一，UTF-8 idea 或事件骨架 |
| `--target-words N` | 目标中文字数，默认 4000 |
| `--overwrite` | 允许覆盖已有正文，沿用正文备份 |
| `--no-context` | 不使用长期记忆上下文 |
| `--pov TEXT` / `--narrative TEXT` | 视角与叙事要求 |
| `--preset general\|compact` | 仅本次生成/预览指定方案，优先于上下文选项及项目默认 |
| `--context-options FILE` | 现有 `ContextOptions` 格式的 JSON 文件 |
| `--prompt-override FILE` | 含 `prompts` 和 `source` 的已确认提示词 JSON |
| `--detach` | 后台提交，立即返回任务状态 |

`novel prompt preview` 接受相同生成参数，构造同一提示词但不调用模型、不写正文。其 JSON 可编辑 `prompts.system` 和 `prompts.user` 后作为 `--prompt-override` 使用；`source` 用于检测模板、材料和输入是否已变更，过期预览会被拒绝。

```sh
novel --json prompt preview ch002 --idea-file idea.txt > preview.json
novel chapter generate ch002 --idea-file idea.txt --prompt-override preview.json
```

默认上下文选项：

```json
{
  "automatic": true,
  "characters": [],
  "chapters": [],
  "events": [],
  "recent_chapters": 2,
  "max_events": 6,
  "input_budget": 8000,
  "include_outline": false,
  "outline_excerpt": ""
}
```

使用 `novel context catalog ch002` 查看可选角色、章节与事件 ID。阶段进度和耗时是实际业务阶段，不是 token 流或预计完成百分比。API 超时、采样参数、关闭思考及订阅路线继续沿用原引擎。

## 其他操作

```sh
novel chapter list
novel chapter save ch001 --file edited.md --overwrite
novel chapter edit ch001 --external
novel chapter title generate ch001
novel chapter title generate ch001 --previous-candidates titles.json
novel chapter title set ch001 "门后的信"
novel chapter reset ch001 --yes
novel chapter reset ch001 --include-memory --yes

novel memory list
novel memory show memory/characters.yaml
novel memory edit memory/characters.yaml
novel memory save outlines/book_outline.yaml --file outline.yaml
novel source show
novel source set 正文

novel settings show
novel settings set llm_provider api
novel settings set llm_base_url http://127.0.0.1:18180/v1
novel settings set llm_model your-model
novel settings set temperature 0.8
novel settings set disable_thinking true
novel settings set max_output_tokens null

novel prompt preset          # 查看项目默认方案
novel prompt preset compact  # 默认使用精简四段式
novel prompt preset general  # 默认使用通用方案
novel prompt show
novel prompt edit
novel prompt save --file prompts.json
novel prompt reset --yes
novel report show ch001 --kind generation
novel report show ch001 --kind consistency
novel doctor
novel doctor --test-model
```

`memory` 覆盖现有允许编辑的世界观、角色、伏笔、关系、时间线、事件、摘要、文风、大纲、作者笔记与项目配置。标题候选参数是一个 JSON 字符串数组；生成候选后用 `title set` 保存选中的名称。

设置展示隐藏 API key。密钥请从菜单中隐藏输入，或通过 `novel settings set llm_api_key --stdin` 的输入流设置，避免写在 shell 参数和历史中。任务请求不保存密钥副本；worker 启动时读取共享凭据。`doctor` 默认只检查环境，显式 `--test-model` 才发请求。

`--stdin` 可替代 `save --file` 读取 UTF-8 正文或记忆文件。Windows PowerShell 5.1 管道编码可能受终端设置影响，长中文内容优先使用 UTF-8 文件或菜单；使用管道前将 `$OutputEncoding` 设置为 UTF-8。

## 脚本约定与诊断

所有命令可加 `--json` 输出 JSON；文本 `show` 默认输出原文，方便重定向。进度和诊断写 stderr。标准命令不弹确认窗口，重置等操作要求显式 `--yes`。

| 退出码 | 含义 |
| --- | --- |
| 0 | 成功 |
| 1 | 业务、模型、文件或配置错误 |
| 2 | 命令格式/参数解析错误 |
| 3 | 项目资源忙或保存内容冲突 |
| 130 | 停止等待或任务安全取消 |

每个子命令均支持 `--help`。后台状态目录包含阶段日志和错误；文件保存部分失败时会返回失败，应检查报告、正文备份和记忆快照后再决定是否重新执行，避免重复记忆更新。

## 直接编辑项目提示词模板

菜单路径：主页 → 提示词与上下文 → 编辑提示词模板（分类）→ 通用方案 / 精简四段式 / 对标方案 / 一致性检查 / 更新记忆。

可以分别直接编辑系统提示词和任务模板，按正常文本换行，Ctrl+S / F2 保存到当前小说项目，重启后仍然有效。`{{事件骨架}}` 等变量按原样保留即可，无需编辑 JSON 或填写转义换行。生成模板必须保留 `{{事件骨架}}`，检查与记忆模板必须保留 `{{章节正文}}`；更新记忆还需要保留程序要求的 JSON 输出结构。

分类菜单还可以查看可用变量、切换生成方案的“追加资料使用原则”、恢复当前分类默认模板。编辑模板不会改变项目默认方案，不会修改其他分类；全量 JSON 编辑/导入入口继续保留用于高级设置。与生成页的临时预览编辑不同，这里修改的是项目永久模板。

## 项目默认生成方案

菜单路径：主页 → 提示词与上下文 → 项目默认生成方案。选择“通用方案”、“精简四段式”或“对标方案”立即保存，退出、重启后继续生效。CLI 与 GUI 在同一小说项目上共享这项设置。

使用独立测试目录时，每条命令都要保留相同的 `--data-dir`，例如：

```sh
./target/debug/novel --data-dir "$HOME/NovelCliTest" prompt preset compact
./target/debug/novel --data-dir "$HOME/NovelCliTest" prompt preset
```

之后普通生成和预览自动跟随项目默认，无需反复选择。`--context-options` 中若显式设置 `generation_preset`，则覆盖项目默认；设为 `null` 或省略可跟随默认。不同数据目录的当前项目记录独立，但若指向同一本书，仍会共用该书目录里的默认方案。

本次命令临时使用另一方案，不改项目默认：

```sh
novel prompt preview ch002 --idea-file idea.txt --preset compact
novel chapter generate ch002 --idea-file idea.txt --preset compact
```

对标方案（`benchmark`）默认是空模板，资料使用原则默认关闭。系统提示词和任务模板可分开保存，也可保存未完成的草稿；预览或生成前必须填写两项内容，并在任务模板中保留 `{{事件骨架}}`。恢复该分类默认模板会清空两项内容。未配置完整的对标模板不影响其他方案使用。

```sh
novel prompt preset benchmark
novel prompt preview ch002 --idea-file idea.txt --preset benchmark
```

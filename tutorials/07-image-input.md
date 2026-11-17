# 07 图片输入：占位符和图片数据如何配对

图片请求有两条并行的数据路径：prompt 里放位置标记，图片本身作为单独的数据传给后端。V4.1 支持这条路径。V4 示例把图片替换为文本 `[Unsupported Image]`。

```text
V4.1 请求：一段文字 + 一张图片
   ├─ Prompt：文字与 <｜image｜> 占位符
   └─ ImageSource：图片来源 → 读取字节 → 预处理 → ImageInfo
                                     ↓
                            后端按占位符顺序接收图片
```

从 [Chat Completions 转换器](../deepseek-recipe/src/protocol/openai/chat_completion/request/convert.rs) 的 `transform_messages` 开始，找图片内容块怎样变成 `ImageSource`。接着读 [ImageResolver](../deepseek-recipe-image/src/resolver.rs) 和 [OpenCV 预处理](../deepseek-recipe-image/src/opencv.rs)。`ImageResolver` 可读取数据 URL 或拉取外部图片；预处理负责解码、尺寸调整和输出图片数据。错误消息会携带图片在请求中的位置，方便定位具体内容块。

## 先认识几个词

| 词 | 在图片请求中的含义 |
| --- | --- |
| Multimodal（多模态） | 同一个请求可以携带不同类型的信息。这里是文字和图片。 |
| Image placeholder（图片占位符） | Prompt 里的特殊标记，用来标出对应图片的位置。 |
| Data URL | 把图片字节编码到字符串里，随请求直接提交。也可以提供外部 URL，由应用读取图片。 |
| 预处理 | 把原始图片解码并调整成后端需要的格式与尺寸。这个项目的 OpenCV 实现会产生处理后尺寸及编码数据。 |
| Quota（配额） | 对同一请求记录已接受的图片数量和字节数。图片 token 数另按预处理后的尺寸计算。 |

例如对话先问「第一张是什么」，再问「第二张是什么」。渲染结果应保留两个占位符的先后顺序。即使第二张先读取完，后端拿到的图片列表仍需按原顺序排列，否则两个位置会对应到错误的图片。

## 为什么要补算 token

带一张图片的 prompt 只为它记录一个占位 token。模型实际处理它可能需要更多 token。假设编码后共有 60 个 ID，其中有一个图片占位符，图片实际消耗 20 个 token；总输入就是 `60 + (20 - 1) = 79`。这是教学数字；项目按处理后的图片尺寸计算 [image_token_adjustment](../deepseek-recipe-core/src/multimodal/mod.rs)。

## 跟读 Rust

先看请求转换用 `model_version.supports_image()` 区分是否保留图片。再读 `RenderedPrompt.image_sources`，通过它找到解析阶段的输入。接着在 `ImageResolver::resolve` 顺序追踪三步。 [Chat Completions 转换](../deepseek-recipe/src/protocol/openai/chat_completion/request/convert.rs) · [渲染结果](../deepseek-recipe-encoding/src/lib.rs) · [图片解析](../deepseek-recipe-image/src/resolver.rs)

1. 校验图片数量，读取或解出原始字节，并检查单张与总字节上限。
2. 创建预处理任务，用 `buffered(concurrency)` 限制同时运行的任务数量，同时保持产出顺序。
3. 等全部图片处理成功后再更新配额。下面是最后一步的源码节选。

```rust
let images = stream::iter(preprocesses)
    .buffered(concurrency)
    .try_collect()
    .await?;
// The call records its sources only once all of them are preprocessed, so
// a failure or a cancellation leaves the quota unchanged.
quota.add(sources.len(), admitted_bytes);
Ok(MultiModalData { images })
```

`quota: &mut ImageQuota` 让函数能更新调用方的配额。`try_collect` 收集成功结果，遇到错误则返回错误。`.await?` 要等到整批成功才继续执行 `quota.add`。随后后端接入方用 `image_token_adjustment` 把每张图超过一个占位 token 的差额加到输入预算中。 [图片差额计算](../deepseek-recipe-core/src/multimodal/mod.rs)

## 练习：只改变模型版本

在 Demo 的 **Render → Chat Completions** 粘贴以下请求。这个 URL 只用于观察渲染，Demo 不会访问它。

```json
{
  "model": "deepseek-flash",
  "messages": [{
    "role": "user",
    "content": [
      {"type": "text", "text": "Hello，这是什么？"},
      {"type": "image_url", "image_url": {"url": "https://example.invalid/sample.png"}}
    ]
  }]
}
```

V4.1 的结果应包含 `<｜image｜>`。把 `model` 改成 `deepseek-v4-pro`，应改为 `[Unsupported Image]`。这个练习能检查协议转换和模板，无需图片文件或 OpenCV。

要接入真实图片处理，从 [Python 示例的 prepare_request](../server-py/main.py) 看如何把 `images` 和额外 token 数传给回调。部署涉及用户提供的外部 URL 时，参考[示例服务器的网络访问约束](../docs/development.md#example-servers)。

## 从“看见占位符”到“图片真正进了模型”

本章的 Demo 练习停留在转换和渲染。真实多模态请求还要依次通过以下边界：

| 可观察结果 | 证明了什么 | 还不能证明什么 |
| --- | --- | --- |
| prompt 含 image 标记 | 模板保留了图片位置 | 图片可下载或可解码 |
| image_sources 数量和顺序正确 | 来源与占位符可配对 | 原始文件内容有效 |
| 得到 ImageInfo | 图片成功预处理 | 引擎使用了相同的图像输入约定 |
| 调整后的输入预算合法 | 当前计数没有超过设定上限 | 模型能正确理解图片 |
| 真实后端回答且传图证据正确 | 已走过完整输入路径 | 回答质量在其他图片上也可靠 |

把图片从 URL 读取与把图片变成模型输入张量区分开。仓库预处理产物如何进入模型，还要按目标引擎接口接线，不能因为 OpenCV 成功就声称完成了视觉模型推理。

### 失败时如何保留正确状态？

假设一个请求有两张图片，第一张成功、第二张解码失败。`try_collect().await?` 会使 resolve 返回错误，后面的 `quota.add` 不执行，因此这次调用不会把部分成功数量写进 quota。这只保证该配额对象的更新语义，不意味着已经发生的网络请求或计算被回滚。

`buffered(concurrency)` 控制预处理并发并保持产出顺序。顺序保证用于占位符配对；并发限制用于控制资源占用。不要为“完成得更快”改用无序输出后直接传给后端。

有 OpenCV 环境后，可选三组实验：一张有效图片；两张可区分的图片；第二张损坏。分别记录 source 位置、预处理结果顺序、token adjustment 或错误路径。这些是进阶验收步骤，本轮没有实际执行图片预处理。

## 自测

一个 prompt 编码后有 60 个 ID，其中 2 个是图片占位符。两张图片实际分别占 20 和 30 个 token，送进后端的总输入是多少？

<details>
<summary>参考答案</summary>

总数是 `60 + (20 - 1) + (30 - 1) = 108`。原来的 60 个 ID 已经为每张图各算了一个占位符，需要补的是差额。

</details>

---

[上一章](06-tools-and-structured-output.md) · [课程目录](README.md) · [下一章](08-contributing.md)

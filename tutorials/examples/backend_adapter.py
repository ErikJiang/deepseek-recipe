"""教学适配器：上游累计文本 → 增量事件。不是某个真实引擎的 SDK。"""
import asyncio
from contextlib import aclosing


def fixture_events():
    # token 数是固定教学数据，不能用它代表真实 tokenizer 的结果。
    return [
        {"kind": "ready", "prompt_tokens": 5},
        {"kind": "snapshot", "text": "Hel", "completion_tokens": 1},
        {"kind": "snapshot", "text": "Hel", "completion_tokens": 1},
        {"kind": "snapshot", "text": "Hello", "completion_tokens": 3},
        {"kind": "finish", "reason": "stop"},
    ]


def count(value):
    if type(value) is not int or value < 0:
        raise ValueError("token 数必须是非负整数")
    return value


async def adapt_events(source):
    """本例约定 source 是有 aclose() 的异步迭代器，snapshot 只能追加。"""
    previous_text = ""
    previous_tokens = 0
    ready = False
    try:
        async for event in source:
            kind = event["kind"]
            if kind == "error":
                raise RuntimeError(event["message"])
            if kind == "ready":
                if ready:
                    raise ValueError("ready 只能发送一次")
                ready = True
                yield {"kind": "ready", "prompt_tokens": count(event["prompt_tokens"])}
            elif kind == "snapshot":
                if not ready:
                    raise ValueError("上游必须先发送 ready")
                text = event["text"]
                tokens = count(event["completion_tokens"])
                if not isinstance(text, str) or not text.startswith(previous_text):
                    raise ValueError("累计文本不再以前文开头，不能当作追加增量")
                if tokens < previous_tokens:
                    raise ValueError("累计 token 数回退")
                delta = text[len(previous_text):]
                token_delta = tokens - previous_tokens
                previous_text, previous_tokens = text, tokens
                if delta or token_delta:
                    yield {"kind": "text", "content": delta, "token_count": token_delta}
            elif kind == "finish":
                if not ready:
                    raise ValueError("上游必须先发送 ready")
                if event["reason"] not in {"stop", "length", "content_filter"}:
                    raise ValueError("未知的结束原因")
                yield {"kind": "finish", "reason": event["reason"]}
                return
            else:
                raise ValueError(f"未知事件类型：{kind}")
        raise RuntimeError("上游缺少显式结束，不能确认请求成功")
    finally:
        await source.aclose()


async def fixture_source():
    for event in fixture_events():
        yield event


async def infer(request):
    """可选 HTTP 桥接；只支持本例的普通非思考文本，不调用真实模型。"""
    from deepseek_recipe import InferenceChunk, InferenceFinishReason, PromptUsage

    if request.conversation_request.conversation.thinking_mode:
        raise ValueError("固定样本仅用于 thinking disabled 的普通文本练习")
    reasons = {
        "stop": InferenceFinishReason.Stop,
        "length": InferenceFinishReason.Length,
        "content_filter": InferenceFinishReason.ContentFilter,
    }
    async with aclosing(adapt_events(fixture_source())) as events:
        async for event in events:
            if event["kind"] == "ready":
                yield InferenceChunk.ready(prompt_usage=PromptUsage(
                    prompt_tokens=event["prompt_tokens"], prompt_cache_hit_tokens=0,
                ))
            elif event["kind"] == "text":
                yield InferenceChunk.text(event["content"], token_count=event["token_count"])
            else:
                yield InferenceChunk.finish(reasons[event["reason"]])


async def demo():
    async with aclosing(adapt_events(fixture_source())) as events:
        async for event in events:
            print(event)


if __name__ == "__main__":
    asyncio.run(demo())

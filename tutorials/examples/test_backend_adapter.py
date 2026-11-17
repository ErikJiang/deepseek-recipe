"""离线适配契约；只依赖标准库，不调用模型或 HTTP 服务。"""
import asyncio
import unittest

from backend_adapter import adapt_events, fixture_events


async def source(events, audit):
    try:
        for event in events:
            yield event
    finally:
        audit.append("closed")


class AdapterTests(unittest.IsolatedAsyncioTestCase):
    async def collect(self, events):
        audit = []
        try:
            return [event async for event in adapt_events(source(events, audit))]
        finally:
            self.assertEqual(audit, ["closed"])

    async def test_cumulative_text_and_counts_become_deltas(self):
        events = await self.collect(fixture_events())
        texts = [event for event in events if event["kind"] == "text"]
        self.assertEqual([event["content"] for event in texts], ["Hel", "lo"])
        self.assertEqual([event["token_count"] for event in texts], [1, 2])
        self.assertEqual(events[-1], {"kind": "finish", "reason": "stop"})

    async def test_rewritten_text_is_rejected(self):
        events = fixture_events()
        events[2] = {"kind": "snapshot", "text": "Help", "completion_tokens": 2}
        with self.assertRaisesRegex(ValueError, "累计文本不再以前文开头"):
            await self.collect(events)

    async def test_decreasing_token_count_is_rejected(self):
        events = fixture_events()
        events[2] = {"kind": "snapshot", "text": "Hel", "completion_tokens": 0}
        with self.assertRaisesRegex(ValueError, "累计 token 数回退"):
            await self.collect(events)

    async def test_missing_finish_is_an_error(self):
        with self.assertRaisesRegex(RuntimeError, "缺少显式结束"):
            await self.collect(fixture_events()[:-1])

    async def test_upstream_error_is_not_a_successful_finish(self):
        with self.assertRaisesRegex(RuntimeError, "upstream failed"):
            await self.collect([{"kind": "error", "message": "upstream failed"}])

    async def test_finish_reasons_are_preserved(self):
        for reason in ["stop", "length", "content_filter"]:
            events = fixture_events()
            events[-1]["reason"] = reason
            result = await self.collect(events)
            self.assertEqual(result[-1]["reason"], reason)

    async def test_snapshot_before_ready_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "先发送 ready"):
            await self.collect(fixture_events()[1:])

    async def test_closing_consumer_closes_source(self):
        audit = []
        stream = adapt_events(source(fixture_events(), audit))
        await anext(stream)
        await stream.aclose()
        self.assertEqual(audit, ["closed"])

    async def test_cancelled_consumer_closes_source(self):
        waiting = asyncio.Event()
        audit = []

        async def waiting_source():
            try:
                yield {"kind": "ready", "prompt_tokens": 5}
                waiting.set()
                await asyncio.Event().wait()
            finally:
                audit.append("closed")

        async def consume():
            return [event async for event in adapt_events(waiting_source())]

        task = asyncio.create_task(consume())
        await waiting.wait()
        task.cancel()
        with self.assertRaises(asyncio.CancelledError):
            await task
        self.assertEqual(audit, ["closed"])


if __name__ == "__main__":
    unittest.main()

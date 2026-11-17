"""需要已安装的 Python bindings、FastAPI 和 OpenCV；见第 10 章。"""
from main import create_app
from backend_adapter import infer

app = create_app(infer=infer)

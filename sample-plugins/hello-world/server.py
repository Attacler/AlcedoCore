import asyncio
import os
from aiohttp import web

from alcedo_sdk import AlcedoClient, KeyNotFoundError, KVStoreError


CORE_URL = os.environ.get("CORE_URL", "http://core:8080")
PORT = os.environ.get("PORT", "8080")

async def _forward_items(method: str, path: str, body: any, query_params: dict | None = None, request_id: str | None = None) -> dict:
    from alcedo_sdk.exceptions import AlcedoError
    try:
        async with AlcedoClient(base_url=CORE_URL, plugin_slug="hello-world", request_id=request_id) as client:
            items_path = path[len("/api/items/"):]
            if method == "GET":
                data = await client.items.list(items_path, params=query_params)
            elif method == "POST":
                data = await client.items.create(items_path, body)
            elif method == "PUT":
                data = await client.items.update(items_path, body)
            elif method == "DELETE":
                data = await client.items.delete(items_path, body)
            else:
                return {"error": f"Unsupported method: {method}"}
            return data
    except AlcedoError as e:
        return {"error": str(e), "status": getattr(e, 'status_code', 500)}


async def get_counter(request_id: str | None = None) -> int:
    try:
        async with AlcedoClient(base_url=CORE_URL, plugin_slug="hello-world", request_id=request_id) as client:
            val = await client.kv.get("counter")
            if val is None:
                return 0
            return int(val)
    except KVStoreError as e:
        print(f"[KV Error] get_counter: {e}")
        return 0


async def decrement_counter(delta: int = 1, request_id: str | None = None) -> int:
    count = await get_counter(request_id=request_id)
    count -= delta
    try:
        async with AlcedoClient(base_url=CORE_URL, plugin_slug="hello-world", request_id=request_id) as client:
            await client.kv.set("counter",str(count))
    except KVStoreError as e:
        print(f"[KV Error] decrement_counter: {e}")
    return count


async def increment_counter(delta: int = 1, request_id: str | None = None) -> int:
    count = await get_counter(request_id=request_id)
    count += delta
    try:
        async with AlcedoClient(base_url=CORE_URL, plugin_slug="hello-world", request_id=request_id) as client:
            res = await client.kv.set("counter",str(count))
            print(res)
    except KVStoreError as e:
        print(f"[KV Error] increment_counter: {e}")
    return count


async def ttl_demo(request_id: str | None = None) -> dict:
    result = {
        "before_set": None,
        "after_set": None,
        "after_expiry": None,
        "status": "ok",
    }
    try:
        demo_key = "ttl-demo"
        async with AlcedoClient(base_url=CORE_URL, plugin_slug="hello-world", request_id=request_id) as client:
            before = await client.kv.ttl(demo_key)
            result["before_set"] = before
            await client.kv.set(demo_key, "This key will auto-expire", ttl=2)
            after_set = await client.kv.ttl(demo_key)
            result["after_set"] = after_set
            val = await client.kv.get(demo_key)
            result["after_set_value"] = val
            await asyncio.sleep(3)
            after_expiry = await client.kv.get(demo_key)
            result["after_expiry_value"] = after_expiry
            after_ttl = await client.kv.ttl(demo_key)
            result["after_ttl"] = after_ttl
    except KVStoreError as e:
        result["status"] = "error"
        result["error"] = str(e)
    return result


async def fetch_kv(key: str, request_id: str | None = None) -> dict:
    try:
        async with AlcedoClient(base_url=CORE_URL, plugin_slug="hello-world", request_id=request_id) as client:
            val = await client.kv.get(key)
            return {"key": key, "value": val}
    except KeyNotFoundError:
        return {"key": key, "value": None, "error": "not_found"}
    except KVStoreError as e:
        return {"key": key, "error": str(e)}


async def change_counter(delta: int, request_id: str | None = None) -> dict:
    func = increment_counter if delta > 0 else decrement_counter
    try:
        count = await func(abs(delta), request_id=request_id)
        return {"count": count}
    except KVStoreError as e:
        return {"error": str(e)}


def _request_id(request: web.Request) -> str | None:
    return request.headers.get("X-Request-ID")


async def _read_body(request: web.Request) -> any:
    try:
        return await request.json()
    except Exception:
        return None


def _json_response(data, status=200):
    return web.json_response(data, status=status)


async def handle_health(request: web.Request) -> web.Response:
    return _json_response({"status": "healthy"})


async def handle_hello(request: web.Request) -> web.Response:
    return _json_response({
        "status": "ok",
        "message": "Hello from plugin API v2.3.1",
        "path": "/api/hello",
        "version": "1.2.3",
    })


async def handle_version(request: web.Request) -> web.Response:
    return _json_response({
        "version": "1.2.3",
        "name": "hello-world",
        "features": ["KV", "DB proxy", "items API", "frontend pages"],
        "status": "upgraded",
    })


async def handle_counter_value(request: web.Request) -> web.Response:
    rid = _request_id(request)
    count = await get_counter(request_id=rid)
    return _json_response({"count": count})


async def handle_ttl_demo(request: web.Request) -> web.Response:
    rid = _request_id(request)
    result = await ttl_demo(request_id=rid)
    return _json_response(result)


async def handle_kv_hello(request: web.Request) -> web.Response:
    rid = _request_id(request)
    result = await fetch_kv("hello", request_id=rid)
    return _json_response(result)


async def handle_items_get(request: web.Request) -> web.Response:
    rid = _request_id(request)
    qp = dict(request.query)
    path = f"/api/items/{request.match_info.get('path', '')}"
    result = await _forward_items("GET", path, None, query_params=qp, request_id=rid)
    status = result.pop("status", 200) if isinstance(result, dict) else 200
    return _json_response(result, status=status)


async def handle_plugin_js(request: web.Request) -> web.Response:
    try:
        with open("pages/dist/plugin-pages.js") as f:
            return web.Response(text=f.read(), content_type="application/javascript")
    except OSError:
        return _json_response({"error": "not_found"}, status=404)


async def handle_root(request: web.Request) -> web.Response:
    return _json_response({
        "status": "ok",
        "message": "Hello from plugin",
        "path": "/",
    })


async def handle_fallback(request: web.Request) -> web.Response:
    return _json_response({"status": "ok", "path": request.path})


async def _parse_delta(request: web.Request) -> int:
    try:
        body = await request.json()
        return int(body.get("delta", body.get("step", 1)))
    except Exception:
        return 1


async def handle_counter_increment(request: web.Request) -> web.Response:
    rid = _request_id(request)
    delta = await _parse_delta(request)
    result = await change_counter(delta, request_id=rid)
    return _json_response(result)


async def handle_counter_decrement(request: web.Request) -> web.Response:
    rid = _request_id(request)
    delta = await _parse_delta(request)
    result = await change_counter(-delta, request_id=rid)
    return _json_response(result)


async def handle_items_post(request: web.Request) -> web.Response:
    rid = _request_id(request)
    body = await _read_body(request)
    path = f"/api/items/{request.match_info.get('path', '')}"
    result = await _forward_items("POST", path, body, request_id=rid)
    status = result.pop("status", 200) if isinstance(result, dict) else 200
    return _json_response(result, status=status)


async def handle_items_put(request: web.Request) -> web.Response:
    rid = _request_id(request)
    body = await _read_body(request)
    path = f"/api/items/{request.match_info.get('path', '')}"
    result = await _forward_items("PUT", path, body, request_id=rid)
    status = result.pop("status", 200) if isinstance(result, dict) else 200
    return _json_response(result, status=status)


async def handle_items_delete(request: web.Request) -> web.Response:
    rid = _request_id(request)
    body = await _read_body(request)
    path = f"/api/items/{request.match_info.get('path', '')}"
    result = await _forward_items("DELETE", path, body, request_id=rid)
    status = result.pop("status", 200) if isinstance(result, dict) else 200
    return _json_response(result, status=status)



def make_app() -> web.Application:
    app = web.Application()

    app.router.add_get("/health", handle_health)
    app.router.add_get("/api/hello", handle_hello)
    app.router.add_get("/api/version", handle_version)
    app.router.add_get("/api/counter/value", handle_counter_value)
    app.router.add_post("/api/counter/increment", handle_counter_increment)
    app.router.add_post("/api/counter/decrement", handle_counter_decrement)
    app.router.add_get("/api/kv/ttl-demo", handle_ttl_demo)
    app.router.add_get("/api/kv/hello", handle_kv_hello)
    app.router.add_get("/", handle_root)

    # Items CRUD via SDK proxy
    items_resource = app.router.add_resource("/api/items/{path:.*}")
    items_resource.add_route("GET", handle_items_get)
    items_resource.add_route("POST", handle_items_post)
    items_resource.add_route("PUT", handle_items_put)
    items_resource.add_route("DELETE", handle_items_delete)

    return app


if __name__ == "__main__":
    app = make_app()
    print("Running on port " + PORT) 
    web.run_app(app, host="0.0.0.0", port=PORT)







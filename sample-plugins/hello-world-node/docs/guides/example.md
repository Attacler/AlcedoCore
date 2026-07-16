# Example: KV Store Workflow

This guide walks through a typical KV store workflow using the Node.js SDK.

## 1. Set a value

```bash
curl -X PUT http://localhost:8080/p/hello-world-node/api/kv/greeting \
  -H "Content-Type: application/json" \
  -d '{"value": "Hello from curl!"}'
```

## 2. Read it back

```bash
curl http://localhost:8080/p/hello-world-node/api/kv/greeting
```

## 3. Add a TTL

```bash
curl -X PUT http://localhost:8080/p/hello-world-node/api/kv/ephemeral \
  -H "Content-Type: application/json" \
  -d '{"value": "gone soon", "ttl": 60}'
```

## 4. List by prefix

```bash
curl "http://localhost:8080/p/hello-world-node/api/kv/list?prefix=greeting"
```

## 5. Batch operations

```bash
curl -X POST http://localhost:8080/p/hello-world-node/api/kv/batch-set \
  -H "Content-Type: application/json" \
  -d '{"pairs": [{"key": "a", "value": 1}, {"key": "b", "value": 2}]}'

curl -X POST http://localhost:8080/p/hello-world-node/api/kv/batch-get \
  -H "Content-Type: application/json" \
  -d '{"keys": ["a", "b"]}'
```

## 6. Delete

```bash
curl -X DELETE http://localhost:8080/p/hello-world-node/api/kv/greeting
```

# API Reference

Complete API documentation for the Hello-World plugin.

## Endpoints

### GET /health

Health check endpoint to verify the plugin is running.

**Response:**

```json
{
  "status": "healthy",
  "version": "1.0.0"
}
```

### GET /hello

Returns a simple greeting message.

**Response:**

```json
{
  "message": "Hello, World!",
  "timestamp": "2024-01-15T10:30:00Z"
}
```

### POST /echo

Echoes back the request body.

**Request Body:**

```json
{
  "message": "Your message here"
}
```

**Response:**

```json
{
  "echo": "Your message here"
}
```

## Error Handling

All endpoints return standard HTTP status codes:

| Status Code | Description |
|-------------|-------------|
| `200` | Success |
| `400` | Bad Request |
| `404` | Not Found |
| `500` | Internal Server Error |

## Rate Limits

- Default: 100 requests per minute
- Burst: 10 requests per second

## Related Documentation

- [Getting Started Guide](./getting-started.md)
- [Example Guide](./guides/example.md)
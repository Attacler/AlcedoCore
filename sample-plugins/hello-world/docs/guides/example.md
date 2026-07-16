# Example Guide

This guide demonstrates nested path support in the documentation structure.

## Overview

The `docs/guides/` directory shows that nested paths are properly supported by the plugin documentation system.

## Common Patterns

### Pattern 1: Configuration Management

```yaml
plugins:
  hello-world:
    enabled: true
    config:
      greeting: "Hello, World!"
```

### Pattern 2: Request/Response Cycle

```python
# Example: Making a request to the hello-world plugin
import requests

response = requests.post(
    'http://localhost:8080/echo',
    json={'message': 'Hello from the guide!'}
)
print(response.json())
```

### Pattern 3: Error Handling

```python
try:
    response = requests.get('http://localhost:8080/invalid')
    response.raise_for_status()
except requests.HTTPError as e:
    print(f"Error: {e}")
```

## Best Practices

1. Always validate input before sending to the plugin
2. Implement proper error handling
3. Use appropriate timeouts for network requests
4. Log all plugin interactions for debugging

## See Also

- [Getting Started](../getting-started.md)
- [API Reference](../api-reference.md)
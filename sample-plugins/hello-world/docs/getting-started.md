# Getting Started with Hello-World Plugin

Welcome to the Hello-World plugin! This guide will help you get up and running quickly.

## Prerequisites

Before you begin, ensure you have:

- Python 3.8 or higher
- Access to the Plugin Microservice Framework
- Basic understanding of REST APIs

## Installation

1. Clone the repository
2. Install dependencies: `pip install -r requirements.txt`
3. Configure your environment variables
4. Run the plugin server

## Quick Start

Here's a simple example to get you started:

```python
from server import app

# Initialize the plugin
app.run(host='0.0.0.0', port=8080)
```

## Configuration

The plugin supports the following configuration options:

| Option | Description | Default |
|--------|-------------|---------|
| `host` | Server host address | `0.0.0.0` |
| `port` | Server port | `8080` |
| `debug` | Enable debug mode | `false` |

## Next Steps

- Read the [API Reference](./api-reference.md) for detailed endpoint documentation
- Check out the [Example Guide](./guides/example.md) for practical examples
# Filter

The overal idea is to support filtering on both the current collection and its relations.<br>
For this we use the following structure:

```ts
{
    ["logic operator"]: [{
        ["field"]: {
            "[operator]": "[value]"
        }
    }]
}
```

| variable       | description                                                                   |     |
| -------------- | ----------------------------------------------------------------------------- | --- |
| logic operator | can be \_and or \_or                                                          |     |
| field          | the api name of the field to filter on                                        |     |
| operator       | the filter operator, for example \_eq (see below for the supported operators) |     |
| value          | the filter value                                                              |     |

For example to fetch a record with the id 1 or with the name "Test": <br>
JSON:

```json
{
    "_or": [
        {
            "id": {
                "_eq": 1
            }
        },
        {
            "name": {
                "_eq": "Test"
            }
        }
    ]
}
```

Query parameter:
`?filter[_or][0][id][_eq]=1&filter[_or][1][name][_eq]=Test`
<br>or:
`?filter={"_or": [{"id": {"_eq": 1}},{"name": {"_eq": "Test"}}]}`

## Supported operators

| operator        | description                           |
| --------------- | ------------------------------------- |
| \_eq            | equals                                |
| \_neq           | not equal                             |
| \_lt            | less than                             |
| \_lte           | less than or equal                    |
| \_gt            | greater than                          |
| \_gte           | greater than or equal                 |
| \_in            | is one of                             |
| \_nin           | is not one of                         |
| \_nin           | is not one of                         |
| \_null          | is null                               |
| \_nnull         | is not null                           |
| \_contains      | contains                              |
| \_ncontains     | doesn't contain                       |
| \_icontains     | contains (case insensitive)           |
| \_starts_with   | starts with                           |
| \_istarts_with  | starts with (case insensitive)        |
| \_nstarts_with  | doesn't starts with                   |
| \_nistarts_with | doesn't start with (case insensitive) |
| \_ends_with     | ends with                             |
| \_iends_with    | ends with (case insensitive)          |
| \_nends_with    | doesn't end with                      |
| \_niends_with   | doesn't end with (case insensitive)   |
| \_between       | between two values                    |
| \_nbetween      | not between two values                |

# Fields

Requesting fields: <br>
`?fields[]=first_name,fields[]=last_name` <br>
Requesting relational fields: <br>
`?fields[]=customer.id,fields[]=customer.email`<br>
Requesting all fields available: <br>
`?fields[]=*,fields[]=customer.*`

# Sort

Sorting based on multiple fields: <br>
`?sort[]=-fieldA&sort[]=+fieldB`.<br>
The - refers to descending and + to ascending.<br>
Providing no - or + will result in ascending.

# Limit

In order to limit the result of your query, you can provide a limit like this:
`?limit=100`
The default limit is 200. 0 can be used to fetch all items.

```

```

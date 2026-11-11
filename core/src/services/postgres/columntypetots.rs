
pub fn column_type_to_ts(
    data_type: String
) -> String {
    match data_type.as_str() {
        // Consolidated numeric types
        "bigint" | "bigserial" | "int" | "int4" | "serial4" | "int2" | "serial2" | "integer"
        | "smallint" | "float" | "float8" | "float4" | "real" | "double precision" => 
            "number".to_string(),
        

        // Decimal types - using f64 for better precision
        "decimal" | "money" => "number".to_string(),

        "boolean" => 
            "boolean".to_string(),
        

        // Text types - only accept strings or explicit conversions
        "text" | "varchar" | "character varying" | "character" | "macaddr" | "macaddr8" | "cidr" | "inet" | "bit"
        | "varbit" | "uuid" => 
            "string".to_string(),
        

        // Date/time types - string only
        "date" | "time" | "timetz" | "timestamp" | "timestamptz" | "interval"
        | "timestamp without time zone" | "timestamp with time zone"
        | "time without time zone" | "time with time zone" => 
            "string".to_string(),
        

        // Geometric types - string only
        "box" | "circle" | "line" | "lseg" | "path" | "point" | "polygon" => 
            "number[]".to_string(),
        

        // Unknown types - return None to make errors visible
        _ => "any".to_string()
    }
}
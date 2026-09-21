/**
 * JSend-style response envelope used by AlcedoCore.
 * See https://github.com/omniti-labs/jsend
 */
export interface JSendResponse<T> {
    status: "success" | "fail" | "error";
    data?: T;
    message?: string;
    code?: number;
}

/**
 * Error thrown by the SDK when the core responds with a JSend `fail`/`error`
 * envelope. The original status/message/code are preserved.
 */
export class AlcedoApiError extends Error {
    public readonly code?: number;
    public readonly status?: "fail" | "error";
    public readonly httpStatus?: number;

    constructor(
        message: string,
        code?: number,
        status?: "fail" | "error",
        httpStatus?: number,
    ) {
        super(message);
        this.name = "AlcedoApiError";
        this.code = code;
        this.status = status;
        this.httpStatus = httpStatus;
    }
}

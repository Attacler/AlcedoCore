import { ref } from 'vue'

export interface UploadedFile {
    id: string
    filename: string
    size_bytes: number
    mime_type: string
}

export interface FileConflict {
    visible: boolean
    filename: string
    file: File
}

function isConflictError(err: any): boolean {
    const response = err?.response || ''
    return typeof response === 'string' && response.includes('already exists')
}

/**
 * File upload with progress tracking and overwrite-conflict handling.
 *
 * When an upload hits a "file already exists" error, `upload()` throws a
 * `FileConflictError` carrying the offending filename. The calling component
 * should catch it, render a confirmation dialog, then call `upload()` again
 * with `overwrite: true` if the user agrees.
 */
export class FileConflictError extends Error {
    filename: string
    file: File
    constructor(filename: string, file: File) {
        super(`A file named '${filename}' already exists`)
        this.filename = filename
        this.file = file
    }
}

export function useFileUpload() {
    const uploading = ref(false)
    const progress = ref(0)

    function uploadWithXhr(
        file: File,
        options: {
            overwrite?: boolean
            folderId?: string
            filename?: string
        },
    ): Promise<UploadedFile> {
        return new Promise((resolve, reject) => {
            const formData = new FormData()
            formData.append('file', file, options.filename || file.name)
            if (options.overwrite) formData.append('overwrite', 'true')
            if (options.folderId) formData.append('folder_id', options.folderId)

            const xhr = new XMLHttpRequest()
            xhr.upload.onprogress = (e) => {
                if (e.lengthComputable)
                    progress.value = Math.round((e.loaded / e.total) * 100)
            }
            xhr.onload = () => {
                if (xhr.status >= 200 && xhr.status < 300) {
                    resolve(JSON.parse(xhr.responseText))
                } else {
                    const err = new Error(xhr.statusText) as any
                    err.status = xhr.status
                    err.response = xhr.responseText
                    reject(err)
                }
            }
            xhr.onerror = () => reject(new Error('Upload failed'))
            xhr.open('POST', '/api/files/upload')
            xhr.send(formData)
        })
    }

    /**
     * Upload a file. Throws a `FileConflictError` when a file with the same
     * name already exists and `overwrite` is not set.
     *
     * `options.filename` overrides the uploaded file's name; `options.folderId`
     * targets a specific media-library folder.
     */
    async function upload(
        file: File,
        options: { overwrite?: boolean; folderId?: string; filename?: string } = {},
    ): Promise<UploadedFile> {
        uploading.value = true
        progress.value = 0
        const effectiveName = options.filename || file.name
        try {
            try {
                return await uploadWithXhr(file, options)
            } catch (err) {
                if (options.overwrite || !isConflictError(err)) throw err
                throw new FileConflictError(effectiveName, file)
            }
        } finally {
            uploading.value = false
        }
    }

    return {
        uploading,
        progress,
        upload,
        FileConflictError,
    }
}

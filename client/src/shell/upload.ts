/**
 * Sending a file to the host: `PUT /files/...` with the file as the request body (the session cookie goes with it,
 * same origin), reporting how much has gone. The Files page's state takes this as a function, so stories and tests
 * pass a fake and never touch the network.
 */

/** How an upload ended: the HTTP status (0 when the request never got an answer) and the host's message. */
export interface UploadAnswer {
  status: number
  message: string
}

/** Send `file` to `url`; `onProgress` hears the fraction sent so far (0 to 1). Never rejects. */
export type Uploader = (
  url: string,
  file: Blob,
  onProgress: (fraction: number) => void
) => Promise<UploadAnswer>

/** The browser's own: `XMLHttpRequest`, the one request that reports upload progress. */
const xhrUpload: Uploader = (url, file, onProgress) =>
  new Promise((resolve) => {
    const xhr = new XMLHttpRequest()
    xhr.open('PUT', url)
    xhr.upload.onprogress = (e) => {
      if (e.lengthComputable && e.total > 0) onProgress(e.loaded / e.total)
    }
    xhr.onload = () => resolve({ status: xhr.status, message: xhr.responseText })
    xhr.onerror = () => resolve({ status: 0, message: 'The connection to vos was lost.' })
    xhr.onabort = () => resolve({ status: 0, message: 'The upload was stopped.' })
    xhr.send(file)
  })

let current: Uploader = xhrUpload

/** The uploader the shell uses now. */
export const uploader: Uploader = (url, file, onProgress) => current(url, file, onProgress)

/** Stories: send uploads to a fake instead of the host. */
export function useUploader(fake: Uploader): void {
  current = fake
}

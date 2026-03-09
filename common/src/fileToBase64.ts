// Copyright © 2026 Jalapeno Labs

export async function fileToBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const fileReader = new FileReader()

    fileReader.onerror = () => {
      reject(new Error('Failed to read selected file'))
    }

    fileReader.onload = () => {
      const dataUrl = fileReader.result
      if (typeof dataUrl !== 'string') {
        reject(new Error('File reader returned an unexpected result'))
        return
      }

      const fileAsBase64 = dataUrl.split(',')[1]
      if (!fileAsBase64?.trim()) {
        reject(new Error('File could not be converted to base64'))
        return
      }

      resolve(fileAsBase64)
    }

    fileReader.readAsDataURL(file)
  })
}

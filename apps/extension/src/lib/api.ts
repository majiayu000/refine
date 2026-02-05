/**
 * Refine 桌面应用 API 客户端
 */

const API_BASE = 'http://localhost:19527'

interface ApiResponse {
  success: boolean
  message?: string
  id?: string
}

/**
 * 检查桌面应用是否运行
 */
export async function checkHealth(): Promise<boolean> {
  try {
    const res = await fetch(`${API_BASE}/health`, {
      method: 'GET',
      mode: 'cors',
    })
    const data: ApiResponse = await res.json()
    return data.success
  } catch {
    return false
  }
}

/**
 * 发送提取的对话到桌面应用
 */
export async function sendToDesktop(
  content: string,
  url: string,
  source: string
): Promise<ApiResponse> {
  try {
    const res = await fetch(`${API_BASE}/extract`, {
      method: 'POST',
      mode: 'cors',
      headers: {
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({ content, url, source }),
    })
    return await res.json()
  } catch (e) {
    return {
      success: false,
      message: '无法连接到桌面应用，请确保 Refine 正在运行',
    }
  }
}

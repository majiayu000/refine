/**
 * Background Service Worker
 *
 * 处理后台任务、与桌面应用通信
 */

// 初始化
chrome.runtime.onInstalled.addListener(() => {
  console.log('Refine extension installed')

  // 初始化 storage
  chrome.storage.local.set({
    stats: { totalItems: 0, todayExtracted: 0 },
    connected: false,
  })
})

// 监听来自 content script 的消息
chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (message.action === 'extracted') {
    // 更新统计
    chrome.storage.local.get(['stats'], (result) => {
      const stats = result.stats || { totalItems: 0, todayExtracted: 0 }
      stats.todayExtracted += 1
      chrome.storage.local.set({ stats })
    })

    sendResponse({ received: true })
  }

  return true
})

// 每天重置统计
chrome.alarms.create('resetDailyStats', {
  periodInMinutes: 60 * 24, // 24 小时
})

chrome.alarms.onAlarm.addListener((alarm) => {
  if (alarm.name === 'resetDailyStats') {
    chrome.storage.local.get(['stats'], (result) => {
      const stats = result.stats || { totalItems: 0, todayExtracted: 0 }
      stats.todayExtracted = 0
      chrome.storage.local.set({ stats })
    })
  }
})

export {}

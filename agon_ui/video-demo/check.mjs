import assert from 'node:assert/strict'
import { chromium, devices, expect } from '@playwright/test'

const origin = process.env.VIDEO_DEMO_URL ?? 'http://127.0.0.1:5173'
const executablePath = process.env.CHROME_BIN
const browser = await chromium.launch({
  executablePath,
  ...(executablePath ? { args: ['--no-sandbox', '--no-zygote', '--single-process'] } : {}),
})
try {
  const page = await browser.newPage({ ...devices['iPhone 13'] })
  page.setDefaultTimeout(10000)
  const errors = []
  page.on('pageerror', (error) => errors.push(error.message))
  await page.goto(`${origin}/matches/demo`)
  await expect(page.getByRole('heading', { name: 'Monday Night Football' })).toBeVisible()
  const panel = page.getByRole('region', { name: 'Video analysis' })
  const link = 'https://www.youtube.com/watch?v=7g9XA8HWn8M'
  const request = page.request
  const unauthorized = await request.post(`${origin}/video-analysis/demo/preview`, { data: { youtube: link } })
  assert.equal(unauthorized.status(), 403)
  const auth = { Authorization: 'Bearer fixture-organiser' }
  const unassociated = await request.post(`${origin}/video-analysis/demo/preview`, {
    data: { youtube: 'https://youtu.be/aaaaaaaaaaa' }, headers: auth,
  })
  assert.equal(unassociated.status(), 404)

  await panel.getByRole('textbox', { name: 'YouTube link' }).fill(link)
  await panel.getByRole('button', { name: 'Check orientation' }).tap()
  const preview = panel.getByAltText('Orientation and goalmouth preview')
  await expect(preview).toBeVisible()
  const upright = await preview.getAttribute('src')
  const rotate = panel.getByRole('button', { name: 'Rotate 90°' })
  for (let i = 0; i < 4; i += 1) {
    await Promise.all([page.waitForResponse((res) => res.url().endsWith('/preview') && res.ok()), rotate.tap()])
    if (i === 1) assert.notEqual(await preview.getAttribute('src'), upright)
  }
  await expect(preview).toHaveAttribute('src', upright)
  await panel.getByRole('button', { name: 'Redraw goal' }).tap()
  for (const [index, [x, y]] of [[0.78, 0.34], [0.78, 0.65], [0.855, 0.65], [0.855, 0.34]].entries()) {
    const corner = panel.getByRole('button', { name: `Mark goal corner ${index + 1}` })
    const bounds = await corner.boundingBox()
    await corner.tap({ position: { x: x * bounds.width, y: y * bounds.height } })
  }
  const analyse = panel.getByRole('button', { name: 'Analyse video' })
  await expect(analyse).toBeDisabled()
  await panel.getByRole('checkbox', { name: 'Frame is upright' }).check()
  const [submitted] = await Promise.all([
    page.waitForResponse((response) => response.url().endsWith('/jobs') && response.request().method() === 'POST'),
    analyse.tap(),
  ])
  assert.equal(submitted.status(), 200)
  const { id } = await submitted.json()
  const jobUrl = `${origin}/video-analysis/demo/jobs/${id}`
  await expect(panel.getByRole('progressbar')).toBeVisible({ timeout: 5000 })
  await expect(panel.getByRole('heading', { name: 'Review candidate goals' })).toBeVisible({ timeout: 90000 })
  const job = await (await request.get(jobUrl, { headers: auth })).json()
  assert.equal(job.result.sample_frames, 480)
  assert.equal(job.result.other_end, 'unobserved')
  assert.equal(job.result.candidates.length, 1)
  assert.equal(job.result.candidates[0].suggested_side, 'blue')
  assert.equal((await request.get(`${origin}/api/matches/demo`).then((response) => response.json())).pending_score, undefined)
  await panel.getByRole('button', { name: 'Inspect footage' }).tap()
  const video = panel.locator('video')
  await expect(video).toBeVisible()
  await expect.poll(() => video.evaluate((element) => Number.isFinite(element.duration) && element.duration), { timeout: 10000 }).toBeGreaterThan(0)
  await video.evaluate((element) => { element.muted = true; return element.play() })
  await expect.poll(() => video.evaluate((element) => element.currentTime)).toBeGreaterThan(0)

  const candidate = panel.getByRole('article')
  await expect(candidate).toContainText('Video 10:16 · match 0:16')
  await candidate.getByRole('button', { name: 'Dismiss' }).tap()
  await expect(candidate).toContainText('dismissed')
  const side = candidate.getByRole('combobox')
  await side.selectOption('blue')
  const confirm = candidate.getByRole('button', { name: 'Confirm goal' })
  for (let i = 0; i < 2; i += 1) {
    const [reviewed] = await Promise.all([
      page.waitForResponse((response) => response.url().endsWith('/review/0') && response.request().method() === 'PUT'),
      confirm.tap(),
    ])
    assert.equal(reviewed.status(), 204)
  }
  await expect(candidate).toContainText('confirmed')
  await page.reload()
  await expect(candidate).toContainText('confirmed')
  assert.equal((await request.get(`${origin}/api/matches/demo`).then((response) => response.json())).pending_score, undefined)
  const openEditor = panel.getByRole('button', { name: 'Open 1 confirmed in result editor' })
  await openEditor.tap()
  const save = page.getByRole('button', { name: 'Save result' })
  await expect(save).toBeDisabled()
  await page.getByRole('checkbox', { name: 'I’ve added any other goals and checked the full match result.' }).check()
  const [saved] = await Promise.all([
    page.waitForResponse((response) => response.url().endsWith('/api/matches/demo') && response.request().method() === 'PATCH'),
    save.tap(),
  ])
  assert.equal(saved.status(), 200)
  const result = (await saved.json()).pending_score.score
  assert.deepEqual(result.score, { blue: 1, red: 0 })
  assert.equal(result.goals.length, 1)
  assert.equal(result.goals[0].minute, 0)
  assert.equal(result.goals[0].occurred_at, undefined)
  await expect(openEditor).toHaveCount(0)
  assert.deepEqual(errors, [])
  console.log(JSON.stringify({ mobile: 'iPhone 13 viewport and touch', ...job.result }, null, 2))
} finally {
  await browser.close()
}

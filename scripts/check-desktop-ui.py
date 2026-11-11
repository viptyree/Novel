"""UI regression checks against Vite with an isolated in-memory Tauri bridge.

Run Vite on port 1420, then: python scripts/check-desktop-ui.py
Requires Python Playwright and its Chromium browser. No project files or model
services are accessed. Screenshots use fictional fixture content.
"""
from pathlib import Path
from playwright.sync_api import sync_playwright, expect

ROOT = Path(__file__).resolve().parents[1]
BRIDGE = r"""
localStorage.setItem('ui.lang', 'zh');
localStorage.setItem('ui.theme', 'light');
let root = 'D:\\Novels\\山海来信';
let directory = '正文';
const chapters = [
  {id:'ch001',title:'旧信与归途',path:'正文/第1章 旧信与归途.md'},
  {id:'ch002',title:'雨夜来客',path:'正文/第2章 雨夜来客.md'},
  {id:'ch003',title:'山海之间',path:'正文/第3章 山海之间.md'}
];
const manuscript = '雨停的时候，林舟已经走到了老屋门前。\n\n门上的铜锁换过了，门槛旁却还留着他小时候刻下的那道划痕。他伸手摸了摸，指尖沾上一层薄灰。\n\n院子里很安静。一棵柿子树越过矮墙，枝头挂着几颗没摘完的果子。\n\n他从口袋里取出那封信。信封的边角已经磨白，正面只有父亲留下的四个字：回家再看。\n\n远处传来渡船的汽笛声。林舟推开门，木轴发出一声低响。';
const texts = Object.fromEntries(chapters.map(c=>[c.id,manuscript]));
const task = {system:'你是一位小说作者。',user_template:'{{事件骨架}}'};
let prompts = { ...task, generation_preset:'general', compact_generation:{...task}, benchmark_generation:{...task},general_material_guidance:true,compact_material_guidance:true,benchmark_material_guidance:true,consistency:{...task},memory:{...task}};
let settings = { project_dir:root,llm_base_url:'http://127.0.0.1:18180/v1',llm_api_key:'local',llm_model:'local-writer',llm_provider:'api',disable_thinking:true};
window.__calls = []; window.__fail = ''; window.__delay = 0;
window.__TAURI_INTERNALS__ = { invoke: async (cmd,args={}) => {
  window.__calls.push({cmd,args});
  if(window.__delay && ['generate_chapter','generate_chapter_manual','read_report'].includes(cmd)) await new Promise(r=>setTimeout(r,window.__delay));
  if(window.__fail===cmd) throw new Error('fixture read/write failure');
  switch(cmd) {
    case 'get_pipeline_root': return root;
    case 'list_chapter_details': return chapters.map(c=>({...c}));
    case 'get_source_settings': return {directory};
    case 'list_projects': return [{path:root,name:'山海来信',current:true,pinned:true,exists:true,last_opened_at:'2026-11-11T09:00:00Z'}, {path:'D:/Novels/Other',name:'另一个项目',current:false,pinned:false,exists:true,last_opened_at:'2026-11-10T09:00:00Z'}];
    case 'open_project': root=args.path; settings.project_dir=root; return root;
    case 'get_settings': return {...settings};
    case 'save_settings': settings={...args.settings}; return;
    case 'save_source_settings': directory=args.settings.directory; return;
    case 'get_prompt_settings': return structuredClone(prompts);
    case 'default_prompt_settings': return structuredClone(prompts);
    case 'save_prompt_settings': prompts=structuredClone(args.settings); return;
    case 'read_chapter': return texts[args.chapterId] || '';
    case 'save_chapter': texts[args.chapterId]=args.content; return;
    case 'context_catalog': return {characters:[{id:'lin',label:'林舟',chapter:1}],chapters:chapters.map((c,i)=>({id:c.id,label:c.title,chapter:i+1})),events:[],chapter_file:'正文/'+args.chapterId+'.md',source_directory:'正文',notes:''};
    case 'preview_generation_prompt': return {prompts:{system:'你是一位小说作者。',user:args.idea},source:'fixture',stats:{characters:100,estimated_tokens:100,budget:8000,over_budget:false,sources:[],warnings:[]}};
    case 'generate_chapter': case 'generate_chapter_manual':
      texts[args.chapterId]=manuscript+'\n\n新的故事，从这里开始。';
      if(!chapters.some(c=>c.id===args.chapterId))chapters.push({id:args.chapterId,title:'',path:args.chapterId+'.md'});
      return {success:true,log:'Completed fixture generation.'};
    case 'generate_chapter_titles': return ['归途', '门后的信', '渡口'];
    case 'save_chapter_title': {const c=chapters.find(c=>c.id===args.chapterId);c.title=args.title;c.path='正文/'+args.title+'.md';return {...c};}
    case 'check_consistency': case 'update_memory': return {success:true,log:'Completed fixture task.'};
    case 'reset_chapter': texts[args.chapterId]='';return {success:true,log:'Reset fixture.'};
    case 'read_memory_file': return 'title: 山海来信\ntheme: 归乡与重逢\n';
    case 'save_memory_file': return;
    case 'read_report': return '# 一致性报告\n\n本章人物与时间线一致。';
    case 'open_url': return;
    default: throw new Error('Unexpected fixture command: '+cmd);
  }
}};
"""


def run():
    with sync_playwright() as pw:
        browser = pw.chromium.launch()
        page = browser.new_page(viewport={"width": 1180, "height": 800}, device_scale_factor=1)
        errors = []
        page.on("pageerror", lambda e: errors.append(str(e)))
        page.add_init_script(BRIDGE)
        page.goto("http://127.0.0.1:1420")
        page.locator(".chapter-item").first.click()
        body = page.locator(".chapter-body")
        expect(body).to_have_value(__import__('re').compile("雨停的时候"))
        page.locator(".idea").fill("林舟打开父亲留下的信，发现一张陌生的地图。")
        for theme in ["light", "dark", "warm"]:
            page.locator(".appearance-menu > summary").click()
            page.locator(".appearance-popover .seg").first.locator("button").nth(["light", "dark", "warm"].index(theme)).click()
            page.locator(".appearance-menu > summary").click()
            page.screenshot(animations="disabled", path=str(ROOT / "docs/screenshots" / f"workbench-{theme}.png"))
            if theme == "light":
                page.screenshot(animations="disabled", path=str(ROOT / "docs/screenshots/workbench.png"))
        # Collapsing panels and focus mode retain both manuscript and AI drafts.
        body.fill("未保存的正文草稿")
        page.locator(".assistant-toggle").click()
        page.locator(".workspace-toolbar .ghost").click()
        expect(page.locator(".sidebar")).not_to_be_visible()
        expect(body).to_have_value("未保存的正文草稿")
        page.locator(".workspace-toolbar .ghost").click()
        page.locator(".assistant-toggle").click()
        expect(page.locator(".idea")).to_have_value("林舟打开父亲留下的信，发现一张陌生的地图。")
        # Cancelling navigation must leave the selected chapter and unsaved text.
        page.once("dialog", lambda dialog: dialog.dismiss())
        page.locator(".chapter-item").nth(1).click()
        expect(body).to_have_value("未保存的正文草稿")
        expect(page.locator(".chapter-item.active")).to_contain_text("旧信与归途")
        # Write errors stay visible with the AI panel closed; drafts survive.
        page.locator(".assistant-toggle").click()
        page.evaluate("window.__fail='save_chapter'")
        page.locator(".manuscript-heading button").click()
        expect(page.locator(".manuscript [role=alert]")).to_contain_text("fixture")
        expect(body).to_have_value("未保存的正文草稿")
        page.evaluate("window.__fail=''")
        page.locator(".manuscript-heading button").click()
        expect(page.locator(".save-state")).to_have_text("已保存")
        page.locator(".assistant-toggle").click()
        page.locator(".generation-footer button").filter(has_text="预览").click()
        expect(page.locator(".prompt-dialog[open]")).to_be_visible()
        page.locator(".prompt-dialog[open] .actions button").last.click()
        page.locator(".idea").fill("新构思，旧预览应失效")
        expect(page.locator(".generation-footer .banner-info")).to_be_visible()
        # Clear stale override, generate, and verify the real busy state.
        page.locator(".generation-footer button").filter(has_text="恢复").click()
        page.evaluate("window.__delay=250")
        page.locator(".generation-footer button.primary").click()
        expect(page.locator(".generation-footer button.primary")).to_be_disabled()
        expect(body).to_have_value(__import__('re').compile("新的故事"))
        page.evaluate("window.__delay=0")
        # Title generation and saving update directory without changing body.
        page.locator(".title-editor > summary").click()
        page.locator(".chapter-title-input button").first.click()
        page.locator(".chapter-title-candidates button").first.click()
        page.locator(".chapter-title-input button").last.click()
        expect(page.locator(".chapter-item.active")).to_contain_text("归途")
        page.locator(".title-editor > summary").click()
        # Manual prompts, budget errors, and cross-page draft retention.
        page.locator(".generation-scroll > .field select").select_option("manual")
        page.locator(".manual-prompt-editor .prompt-user").fill("手动提示词草稿")
        page.locator(".manual-prompt-editor input[type=number]").fill("1")
        expect(page.locator(".generation-footer button.primary")).to_be_disabled()
        page.locator(".nav").nth(2).click()
        page.locator("textarea.code").fill("author draft")
        page.screenshot(animations="disabled", path=str(ROOT / "docs/screenshots/memory.png"))
        page.locator(".nav").nth(3).click()
        page.locator(".memory-edit .prompt-system").fill("提示词草稿")
        page.locator(".nav").nth(4).click()
        page.screenshot(animations="disabled", path=str(ROOT / "docs/screenshots/settings.png"))
        page.locator(".nav").nth(5).click()
        expect(page.locator(".guide-toc a").first).to_be_visible()
        page.locator(".nav").nth(2).click()
        expect(page.locator("textarea.code")).to_have_value("author draft")
        page.locator(".nav").nth(3).click()
        expect(page.locator(".memory-edit .prompt-system")).to_have_value("提示词草稿")
        page.locator(".nav").first.click()
        expect(page.locator(".manual-prompt-editor .prompt-user")).to_have_value("手动提示词草稿")
        page.locator(".manual-prompt-editor input[type=number]").fill("8000")
        page.locator(".generation-footer button.primary").click()
        expect(page.locator(".generation-footer button.primary")).to_be_enabled()
        # Existing consistency, memory update and confirmed reset remain reachable.
        page.locator(".chapter-tools > summary").click()
        page.locator(".chapter-tools > .actions button").first.click()
        page.locator(".chapter-tools > .actions button").last.click()
        page.locator(".danger-zone > summary").click()
        page.once("dialog", lambda dialog: dialog.accept())
        page.locator(".danger-zone button").click()
        expect(body).to_have_value("")
        # Reports distinguish errors, emptiness and successful reads.
        page.locator(".nav").nth(1).click()
        expect(page.locator(".reports .markdown")).to_contain_text("一致性报告")
        page.evaluate("window.__fail='read_report'")
        page.locator(".reports-bar > button").click()
        expect(page.locator(".reports [role=alert]")).to_contain_text("报告读取失败")
        page.evaluate("window.__fail=''")
        page.locator(".nav").first.click()
        # At minimum desktop size the assistant is an accessible drawer.
        page.set_viewport_size({"width": 900, "height": 640})
        expect(page.locator(".assistant-panel")).not_to_be_visible()
        page.locator(".assistant-toggle").click()
        expect(page.get_by_role("dialog", name="AI 辅助")).to_be_visible()
        expect(page.locator(".manuscript")).to_have_attribute("inert", "")
        page.keyboard.press("Shift+Tab")
        assert page.evaluate("document.querySelector('.assistant-panel').contains(document.activeElement)")
        page.keyboard.press("Escape")
        expect(page.locator(".assistant-toggle")).to_be_focused()
        for width, height in [(900,640),(1180,800),(944,640),(787,533)]:
            page.set_viewport_size({"width":width,"height":height})
            assert page.evaluate("document.documentElement.scrollWidth <= innerWidth"), f"Page overflow at {width}"
        page.set_viewport_size({"width":1180,"height":800})
        page.locator(".appearance-menu > summary").click()
        page.locator(".appearance-popover .seg").last.locator("button").last.click()
        page.locator(".appearance-menu > summary").click()
        expect(page.locator("html")).to_have_attribute("lang", "en")
        assert not errors, errors
        # New-chapter generation uses the selected ID and is reachable without a dropdown.
        # Search supports title filtering and no-results states.
        page.locator(".chapter-search input").fill("不存在的章名")
        expect(page.locator(".chapter-item")).to_have_count(0)
        expect(page.locator(".empty-hint")).to_be_visible()
        page.locator(".chapter-search input").fill("")
        page.locator(".new-chapter > summary").click()
        page.locator(".new-chapter input").fill("ch004")
        page.once("dialog", lambda dialog: dialog.accept())
        page.locator(".new-chapter button.primary").click()
        page.locator(".generation-scroll > .field select").select_option("template")
        page.locator(".generation-footer button.primary").click()
        expect(page.locator(".chapter-item.active")).to_contain_text("ch004")
        commands = page.evaluate("window.__calls.map(c=>c.cmd)")
        for command in ["save_chapter", "preview_generation_prompt", "generate_chapter", "generate_chapter_manual", "save_chapter_title", "check_consistency", "update_memory", "reset_chapter"]:
            assert command in commands, command
        # Busy project changes are blocked; unsaved project switches can be cancelled.
        page.evaluate("window.__delay=600")
        page.locator(".generation-footer button.primary").click()
        page.locator(".chapter-directory .project-trigger").click()
        page.locator(".project-entry button.primary").last.click()
        expect(page.locator(".project-dialog [role=alert]")).to_be_visible()
        page.locator(".project-dialog-header button").click()
        expect(page.locator(".generation-footer button.primary")).to_be_enabled()
        page.evaluate("window.__delay=0")
        body.fill("project switch draft")
        page.locator(".chapter-directory .project-trigger").click()
        page.once("dialog", lambda dialog: dialog.dismiss())
        page.locator(".project-entry button.primary").last.click()
        expect(body).to_have_value("project switch draft")
        page.once("dialog", lambda dialog: dialog.accept())
        page.locator(".project-entry button.primary").last.click()
        expect(page.locator(".project-dialog")).not_to_be_visible()
        expect(body).not_to_be_visible()
        page.locator(".chapter-item").first.click()
        body.fill("source change draft")
        page.locator(".nav").nth(4).click()
        page.locator('.settings input[placeholder="正文 / chapters"]').fill("manuscripts")
        page.once("dialog", lambda dialog: dialog.dismiss())
        page.locator(".settings .actions button.primary").click()
        assert page.evaluate("window.__calls.filter(c=>c.cmd==='save_source_settings').length") == 0
        page.locator(".nav").first.click()
        expect(body).to_have_value("source change draft")
        assert not errors, errors
        print("PASS: workspace layout, themes, drafts, navigation guards, save failures, preview, generation, titles, tools, pages, drawer keyboard, scaling, language and new chapter")
        browser.close()


if __name__ == "__main__":
    run()

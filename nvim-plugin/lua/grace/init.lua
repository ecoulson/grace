local curl = require('plenary.curl')
local popup = require('plenary.popup')

Grace_window_id = nil
Indexer_URL = 'https://ckgjsixa23.execute-api.us-west-2.amazonaws.com/production/indexer'

local GraceModule = {}

function GraceModule.get_issues()
    local line = vim.api.nvim_get_current_line()

    local issues_response = curl.post(Indexer_URL, {
        timeout = 2000,
        body = {
            line,
        }
    })

    return issues_response
end

function GraceModule.display_issues()
    local response = GraceModule.get_issues()
    local payload = vim.json.decode(response.body)
    local buffer = vim.api.nvim_create_buf(false, false)
    local window_id, window = popup.create(buffer, {
        title = 'Grace',
        highlight = "GraceWindow",
        minwidth = 60,
        minheight = 10,
        col = math.floor(((vim.o.lines - 10) / 2) - 1),
        row = math.floor((vim.o.columns - 60) / 2),
        borderchars = { "─", "│", "─", "│", "╭", "╮", "╯", "╰" }
    })
    local close_command = "lua require('grace').close()"

    Grace_window_id = window_id

    vim.api.nvim_win_set_option(window_id, "number", true)
    vim.api.nvim_buf_set_name(buffer, "grace-menu")
    vim.api.nvim_buf_set_option(buffer, "filetype", "grace")
    vim.api.nvim_buf_set_option(buffer, "buftype", "nowrite")
    vim.api.nvim_buf_set_option(buffer, "bufhidden", "delete")
    vim.api.nvim_buf_set_lines(buffer, 0, #payload, false, payload)
    vim.api.nvim_win_set_option(window.border.win_id, "winhl", "Normal:GraceWindow")
    vim.api.nvim_buf_set_keymap(buffer, "n", "q", string.format("<Cmd>%s<CR>", close_command), {
        silent = true
    })
    vim.api.nvim_buf_set_keymap(buffer, "n", "<ESC>", string.format("<Cmd>%s<CR>", close_command), {
        silent = true
    })
    vim.cmd(string.format("autocmd BufLeave <buffer> ++nested ++once silent %s", close_command))
end

function GraceModule.close()
    vim.api.nvim_win_close(Grace_window_id, true)
    Grace_window_id = nil
end

return GraceModule

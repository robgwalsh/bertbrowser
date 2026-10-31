// The operations UI: the Transfers window and its queue, the clash dialog, the offer of administrator
// rights, the History window, rename (plain and advanced), compress, extract and the archive password.
// Everything they show is a model the core hands back (`bb_ops_query`) and everything they do is a verb
// (`bb_ops_do`, or the operation exports), so none of it keeps a second copy of the truth.
#pragma once
#include "views/bridge/core.h"
#include "views/dialogs/dialogs.h"
#include "views/bridge/json.h"

#include <map>
#include <string>
#include <vector>

namespace bb::views::ops {

// A JSON string literal / array of them, from wide text.
std::string q(const std::wstring& text);
std::string qa(const std::vector<std::wstring>& items);

// `bb_ops_query` / `bb_ops_do`, parsed. A failed call throws core::CoreError.
json::Value query(core::Core& core, const std::string& what, const std::string& arg = {});
json::Value run(core::Core& core, const std::string& verb, const std::string& arg = {});

// The kinds this file builds, for the dialog table.
void add_dialogs(std::map<std::wstring, DialogBuilder>& table);

}  // namespace bb::views::ops

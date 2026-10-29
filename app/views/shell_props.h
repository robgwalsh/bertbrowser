// Shell property columns: the core asks for cells ([[path, canonical name], ...]) and this answers with what
// the Windows property system says for each, formatted for display. Read through the keyed path
// (IPropertyStore on the file), never the enumerate-everything one a Properties dialog uses.
#pragma once
#include "views/json.h"

#include <string>
#include <utility>
#include <vector>

namespace bb::views {

// `requests` is the array bb_props_requests returns. The result is the JSON bb_props_supply takes:
// [{path, canonical, value?: {display, number?, date?}}], where a missing value means the handler had none.
std::string read_property_cells(const json::Value& requests);

// Every property the shell has a value for on one file, named and formatted for display, sorted by name:
// the Properties dialog's Details. The enumerate-everything read, so only ever for one file the core has
// said may be opened (never a cloud placeholder). Empty when the handler refuses or misbehaves.
std::vector<std::pair<std::wstring, std::wstring>> read_all_properties(const std::wstring& path);

// The properties this machine reports, as the JSON bb_columns_set_machine_properties takes ([[canonical,
// display name], ...]); an empty array when the property system cannot be enumerated.
std::string machine_property_list();

}  // namespace bb::views

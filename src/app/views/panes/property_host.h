// The core's window onto the Windows property system, which only this side can reach: what a file's handler
// says about it (the details under a picture), which properties it will write and what they hold now (for
// a file no codec handles), and the write itself. The core calls these from whichever thread needs the
// answer, so each initialises COM for the thread it is called on and returns text that stays valid until the
// next call on that thread.
#pragma once
#include "bbcore.h"

namespace bb::views {

BbPropertyHost make_property_host();

}  // namespace bb::views

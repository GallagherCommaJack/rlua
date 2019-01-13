use std::marker::PhantomData;
use std::{ptr, slice, str};

use libc::{self, c_int};

use crate::context::Context;
use crate::ffi::{self, lua_Debug, lua_State};
use crate::lua::extra_data;
use crate::util::callback_error;

/// Contains information about currently executing Lua code.
///
/// The `Debug` structure is provided as a parameter to the hook function set with
/// [`Lua::set_hook`].  You may call the methods on this structure to retrieve information about the
/// Lua code executing at the time that the hook function was called.  Further information can be
/// found in the [Lua 5.3 documentaton][lua_doc].
///
/// [lua_doc]: https://www.lua.org/manual/5.3/manual.html#lua_Debug
/// [`Lua::set_hook`]: struct.Lua.html#method.set_hook
#[derive(Clone)]
pub struct Debug<'a> {
    ar: *mut lua_Debug,
    state: *mut lua_State,
    _phantom: PhantomData<&'a ()>,
}

impl<'a> Debug<'a> {
    /// Corresponds to the `n` what mask.
    pub fn names(&self) -> DebugNames<'a> {
        unsafe {
            rlua_assert!(
                ffi::lua_getinfo(self.state, cstr!("n"), self.ar) != 0,
                "lua_getinfo failed with `n`"
            );
            DebugNames {
                name: ptr_to_str((*self.ar).name as *const i8),
                name_what: ptr_to_str((*self.ar).namewhat as *const i8),
            }
        }
    }

    /// Corresponds to the `n` what mask.
    pub fn source(&self) -> DebugSource<'a> {
        unsafe {
            rlua_assert!(
                ffi::lua_getinfo(self.state, cstr!("S"), self.ar) != 0,
                "lua_getinfo failed with `S`"
            );
            DebugSource {
                source: ptr_to_str((*self.ar).source as *const i8),
                short_src: ptr_to_str((*self.ar).short_src.as_ptr() as *const i8),
                line_defined: (*self.ar).linedefined as i32,
                last_line_defined: (*self.ar).lastlinedefined as i32,
                what: ptr_to_str((*self.ar).what as *const i8),
            }
        }
    }

    /// Corresponds to the `l` what mask. Returns the current line.
    pub fn curr_line(&self) -> i32 {
        unsafe {
            rlua_assert!(
                ffi::lua_getinfo(self.state, cstr!("l"), self.ar) != 0,
                "lua_getinfo failed with `l`"
            );
            (*self.ar).currentline as i32
        }
    }

    /// Corresponds to the `t` what mask. Returns true if the hook is in a function tail call, false
    /// otherwise.
    pub fn is_tail_call(&self) -> bool {
        unsafe {
            rlua_assert!(
                ffi::lua_getinfo(self.state, cstr!("t"), self.ar) != 0,
                "lua_getinfo failed with `t`"
            );
            (*self.ar).currentline != 0
        }
    }

    /// Corresponds to the `u` what mask.
    pub fn stack(&self) -> DebugStack {
        unsafe {
            rlua_assert!(
                ffi::lua_getinfo(self.state, cstr!("u"), self.ar) != 0,
                "lua_getinfo failed with `u`"
            );
            DebugStack {
                num_ups: (*self.ar).nups as i32,
                num_params: (*self.ar).nparams as i32,
                is_vararg: (*self.ar).isvararg != 0,
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct DebugNames<'a> {
    pub name: Option<&'a [u8]>,
    pub name_what: Option<&'a [u8]>,
}

#[derive(Clone, Debug)]
pub struct DebugSource<'a> {
    pub source: Option<&'a [u8]>,
    pub short_src: Option<&'a [u8]>,
    pub line_defined: i32,
    pub last_line_defined: i32,
    pub what: Option<&'a [u8]>,
}

#[derive(Copy, Clone, Debug)]
pub struct DebugStack {
    pub num_ups: i32,
    pub num_params: i32,
    pub is_vararg: bool,
}

/// Indicates in what circumstances the hook function should be called by Lua.
#[derive(Clone, Debug, Default)]
pub struct HookTriggers {
    /// Before a function call.
    pub on_calls: bool,
    /// When Lua returns from a function.
    pub on_returns: bool,
    /// Before executing a new line, or returning from a function call.
    pub every_line: bool,
    /// After a certain amount of instructions. When set to `Some(count)`, `count` is the number of
    /// instructions to execute before calling the hook.
    ///
    /// # Performance
    ///
    /// Setting this option to a low value can have a very high overhead.
    pub every_nth_instruction: Option<u32>,
}

impl HookTriggers {
    // Compute the mask to pass to `lua_sethook`.
    pub(crate) fn mask(&self) -> c_int {
        let mut mask: c_int = 0;
        if self.on_calls {
            mask |= ffi::LUA_MASKCALL
        }
        if self.on_returns {
            mask |= ffi::LUA_MASKRET
        }
        if self.every_line {
            mask |= ffi::LUA_MASKLINE
        }
        if self.every_nth_instruction.is_some() {
            mask |= ffi::LUA_MASKCOUNT
        }
        mask
    }

    // Returns the `count` parameter to pass to `lua_sethook`, if applicable. Otherwise, zero is
    // returned.
    pub(crate) fn count(&self) -> c_int {
        self.every_nth_instruction.unwrap_or(0) as c_int
    }
}

// This callback is passed to `lua_sethook` and gets called whenever debug information is received.
pub(crate) unsafe extern "C" fn hook_proc(state: *mut lua_State, ar: *mut lua_Debug) {
    callback_error(state, || {
        let context = Context::new(state);

        let debug = Debug {
            ar,
            state,
            _phantom: PhantomData,
        };

        let cb = rlua_expect!(
            (*extra_data(state)).hook_callback.clone(),
            "no hook callback set in hook_proc"
        );
        let outcome = match cb.try_borrow_mut() {
            Ok(mut b) => (&mut *b)(context, debug),
            Err(_) => rlua_panic!("Lua should not allow hooks to be called within another hook"),
        };
        outcome
    });
}

unsafe fn ptr_to_str<'a>(input: *const i8) -> Option<&'a [u8]> {
    if input == ptr::null() || ptr::read(input) == 0 {
        None
    } else {
        let len = libc::strlen(input) as usize;
        Some(slice::from_raw_parts(input as *const u8, len))
    }
}
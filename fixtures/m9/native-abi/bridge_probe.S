/* ARM64 oracle targets: Python models worst-case host callback clobbers.
 * This links the production native_call.S unchanged. MIT OR Apache-2.0. */
.text
.global as_native_errno_enter
as_native_errno_enter: ret
.global as_native_errno_leave
as_native_errno_leave: ret
.global as_native_trap
as_native_trap: brk #0
.global oracle_host_target
oracle_host_target: ret
.global oracle_inner_target
oracle_inner_target: ret
.global oracle_nested_return
oracle_nested_return: ret
.global oracle_inner_guest
oracle_inner_guest:
  stp x29,x30,[sp,#-16]!
  adr x16,oracle_inner_target
  adr x17,as_native_import_bridge
  blr x17
  ldp x29,x30,[sp],#16
  ret

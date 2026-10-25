/* Native Android LP64 Vulkan discovery/device contracts over real Mesa NVK.
 * No Android WSI/presentation/queue submission or guest allocation/debug
 * callbacks are advertised by this initial bounded bridge.
 * AndroSwitch contributors, MIT OR Apache-2.0. */
#include "vulkan_bridge.h"
#ifdef ANDROSWITCH_MESA_UNIFIED
#include <errno.h>
#include <stddef.h>
#include <string.h>
#include <switch.h>
#include <vulkan/vulkan.h>

#include "bionic_dlfcn.h"
#include "native.h"
static Mutex lock;
static VkInstance instances[16];
static struct {
  VkPhysicalDevice handle;
  VkInstance instance;
} physicals[64];
static struct {
  VkDevice handle;
  VkInstance instance;
} devices[32];
static PFN_vkVoidFunction guest_instance_proc(VkInstance, const char *);
static PFN_vkVoidFunction guest_device_proc(VkDevice, const char *);
static void unsupported_callbacks(void) { as_native_trap(-2350); }
static VkResult guest_create_instance(const VkInstanceCreateInfo *info,
                                      const VkAllocationCallbacks *allocator,
                                      VkInstance *output) {
  if (allocator || (info && info->pNext)) {
    unsupported_callbacks();
    return VK_ERROR_INITIALIZATION_FAILED;
  }
  mutexLock(&lock);
  size_t i = 0;
  while (i < 16 && instances[i]) i++;
  if (i == 16) {
    mutexUnlock(&lock);
    return VK_ERROR_OUT_OF_HOST_MEMORY;
  }
  VkResult rc = vkCreateInstance(info, NULL, output);
  if (rc == VK_SUCCESS) instances[i] = *output;
  mutexUnlock(&lock);
  return rc;
}
static void guest_destroy_instance(VkInstance instance,
                                   const VkAllocationCallbacks *allocator) {
  if (allocator) {
    unsupported_callbacks();
    return;
  }
  if (!instance) return;
  mutexLock(&lock);
  size_t i = 0;
  while (i < 16 && instances[i] != instance) i++;
  int busy = 0;
  for (size_t d = 0; d < 32; d++)
    if (devices[d].handle && devices[d].instance == instance) busy = 1;
  if (i == 16 || busy) {
    mutexUnlock(&lock);
    as_native_trap(-2351);
    return;
  }
  vkDestroyInstance(instance, NULL);
  instances[i] = VK_NULL_HANDLE;
  for (size_t p = 0; p < 64; p++)
    if (physicals[p].instance == instance)
      memset(&physicals[p], 0, sizeof(physicals[p]));
  mutexUnlock(&lock);
}
static VkResult guest_enumerate(VkInstance instance, uint32_t *count,
                                VkPhysicalDevice *output) {
  if (!count) return VK_ERROR_INITIALIZATION_FAILED;
  if (output && *count > 64) return VK_ERROR_OUT_OF_HOST_MEMORY;
  mutexLock(&lock);
  VkResult rc = vkEnumeratePhysicalDevices(instance, count, output);
  if (output && (rc == VK_SUCCESS || rc == VK_INCOMPLETE))
    for (uint32_t i = 0; i < *count; i++) {
      size_t p = 0;
      while (p < 64 && physicals[p].handle && physicals[p].handle != output[i])
        p++;
      if (p == 64) {
        mutexUnlock(&lock);
        return VK_ERROR_OUT_OF_HOST_MEMORY;
      }
      physicals[p].handle = output[i];
      physicals[p].instance = instance;
    }
  mutexUnlock(&lock);
  return rc;
}
static VkResult guest_create_device(VkPhysicalDevice physical,
                                    const VkDeviceCreateInfo *info,
                                    const VkAllocationCallbacks *allocator,
                                    VkDevice *output) {
  if (allocator || (info && info->pNext)) {
    unsupported_callbacks();
    return VK_ERROR_INITIALIZATION_FAILED;
  }
  if (info)
    for (uint32_t i = 0; i < info->queueCreateInfoCount; i++)
      if (info->pQueueCreateInfos[i].pNext) {
        unsupported_callbacks();
        return VK_ERROR_INITIALIZATION_FAILED;
      }
  mutexLock(&lock);
  size_t p = 0;
  while (p < 64 && physicals[p].handle != physical) p++;
  size_t i = 0;
  while (i < 32 && devices[i].handle) i++;
  if (p == 64 || i == 32) {
    mutexUnlock(&lock);
    return VK_ERROR_OUT_OF_HOST_MEMORY;
  }
  VkResult rc = vkCreateDevice(physical, info, NULL, output);
  if (rc == VK_SUCCESS) {
    devices[i].handle = *output;
    devices[i].instance = physicals[p].instance;
  }
  mutexUnlock(&lock);
  return rc;
}
static void guest_destroy_device(VkDevice device,
                                 const VkAllocationCallbacks *allocator) {
  if (allocator) {
    unsupported_callbacks();
    return;
  }
  if (!device) return;
  mutexLock(&lock);
  size_t i = 0;
  while (i < 32 && devices[i].handle != device) i++;
  if (i == 32) {
    mutexUnlock(&lock);
    as_native_trap(-2351);
    return;
  }
  vkDestroyDevice(device, NULL);
  memset(&devices[i], 0, sizeof(devices[i]));
  mutexUnlock(&lock);
}
#define PROCEDURE(n, f) \
  if (!strcmp(name, n)) return (PFN_vkVoidFunction)(f)
static PFN_vkVoidFunction exposed(const char *name) {
  PROCEDURE("vkGetInstanceProcAddr", guest_instance_proc);
  PROCEDURE("vkGetDeviceProcAddr", guest_device_proc);
  PROCEDURE("vkCreateInstance", guest_create_instance);
  PROCEDURE("vkDestroyInstance", guest_destroy_instance);
  PROCEDURE("vkEnumeratePhysicalDevices", guest_enumerate);
  PROCEDURE("vkCreateDevice", guest_create_device);
  PROCEDURE("vkDestroyDevice", guest_destroy_device);
  PROCEDURE("vkEnumerateInstanceVersion", vkEnumerateInstanceVersion);
  PROCEDURE("vkEnumerateInstanceExtensionProperties",
            vkEnumerateInstanceExtensionProperties);
  PROCEDURE("vkEnumerateInstanceLayerProperties",
            vkEnumerateInstanceLayerProperties);
  PROCEDURE("vkGetPhysicalDeviceProperties", vkGetPhysicalDeviceProperties);
  PROCEDURE("vkGetPhysicalDeviceQueueFamilyProperties",
            vkGetPhysicalDeviceQueueFamilyProperties);
  PROCEDURE("vkGetPhysicalDeviceMemoryProperties",
            vkGetPhysicalDeviceMemoryProperties);
  PROCEDURE("vkGetPhysicalDeviceFeatures", vkGetPhysicalDeviceFeatures);
  PROCEDURE("vkEnumerateDeviceExtensionProperties",
            vkEnumerateDeviceExtensionProperties);
  PROCEDURE("vkGetDeviceQueue", vkGetDeviceQueue);
  PROCEDURE("vkDeviceWaitIdle", vkDeviceWaitIdle);
  return NULL;
}
static PFN_vkVoidFunction guest_instance_proc(VkInstance instance,
                                              const char *name) {
  if (!name || !vkGetInstanceProcAddr(instance, name)) return NULL;
  PFN_vkVoidFunction target = exposed(name);
  return target
             ? (PFN_vkVoidFunction)as_bionic_function_veneer((uint64_t)target)
             : NULL;
}
static PFN_vkVoidFunction guest_device_proc(VkDevice device, const char *name) {
  if (!name || !vkGetDeviceProcAddr(device, name)) return NULL;
  PFN_vkVoidFunction target = exposed(name);
  return target
             ? (PFN_vkVoidFunction)as_bionic_function_veneer((uint64_t)target)
             : NULL;
}
uint64_t as_vulkan_import(const char *name) { return (uint64_t)exposed(name); }
int as_vulkan_reset(void) {
  mutexLock(&lock);
  for (size_t i = 0; i < 32; i++)
    if (devices[i].handle) {
      if (vkDeviceWaitIdle(devices[i].handle) != VK_SUCCESS) {
        mutexUnlock(&lock);
        return -EIO;
      }
      vkDestroyDevice(devices[i].handle, NULL);
      memset(&devices[i], 0, sizeof(devices[i]));
    }
  for (size_t i = 0; i < 16; i++)
    if (instances[i]) {
      vkDestroyInstance(instances[i], NULL);
      instances[i] = VK_NULL_HANDLE;
    }
  memset(physicals, 0, sizeof(physicals));
  mutexUnlock(&lock);
  return 0;
}
#else
uint64_t as_vulkan_import(const char *name) {
  (void)name;
  return 0;
}
int as_vulkan_reset(void) { return 0; }
#endif

/* Optional actual Android Vulkan ABI probes; no fabricated GPU/device. */
#include <dlfcn.h>
#include <jni.h>
#include <stdio.h>
#include <string.h>
#include <vulkan/vulkan.h>
static jstring result(JNIEnv *e, const char *s) {
  return (*e)->NewStringUTF(e, s);
}
jstring as_probe_vulkan(JNIEnv *e, int mode) {
  void *lib = dlopen("libvulkan.so", RTLD_NOW);
  if (!lib)
    return result(e,
                  "UNSUPPORTED:Android Vulkan library/bridge is not available");
  PFN_vkGetInstanceProcAddr get =
      (PFN_vkGetInstanceProcAddr)dlsym(lib, "vkGetInstanceProcAddr");
  if (!get) return result(e, "UNSUPPORTED:vkGetInstanceProcAddr");
  if (mode == 0)
    return result(e,
                  "Vulkan Android entrypoint available; not a rendering pass");
  PFN_vkCreateInstance create =
      (PFN_vkCreateInstance)get(VK_NULL_HANDLE, "vkCreateInstance");
  if (!create) return result(e, "FAIL:vkCreateInstance missing");
  VkApplicationInfo app = {.sType = VK_STRUCTURE_TYPE_APPLICATION_INFO,
                           .pApplicationName = "AndroSwitch Conformance",
                           .apiVersion = VK_API_VERSION_1_0};
  VkInstanceCreateInfo info = {.sType = VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO,
                               .pApplicationInfo = &app};
  VkInstance instance = VK_NULL_HANDLE;
  VkResult rc = create(&info, NULL, &instance);
  if (rc != VK_SUCCESS) {
    char detail[96];
    snprintf(detail, sizeof(detail), "%s:vkCreateInstance result=%d",
             rc == VK_ERROR_INCOMPATIBLE_DRIVER ? "UNSUPPORTED" : "FAIL", rc);
    return result(e, detail);
  }
  PFN_vkDestroyInstance destroy =
      (PFN_vkDestroyInstance)get(instance, "vkDestroyInstance");
  PFN_vkEnumeratePhysicalDevices enumerate =
      (PFN_vkEnumeratePhysicalDevices)get(instance,
                                          "vkEnumeratePhysicalDevices");
  PFN_vkGetPhysicalDeviceProperties properties =
      (PFN_vkGetPhysicalDeviceProperties)get(instance,
                                             "vkGetPhysicalDeviceProperties");
  PFN_vkGetPhysicalDeviceQueueFamilyProperties queues =
      (PFN_vkGetPhysicalDeviceQueueFamilyProperties)get(
          instance, "vkGetPhysicalDeviceQueueFamilyProperties");
  PFN_vkGetPhysicalDeviceMemoryProperties memory =
      (PFN_vkGetPhysicalDeviceMemoryProperties)get(
          instance, "vkGetPhysicalDeviceMemoryProperties");
  PFN_vkCreateDevice device_create =
      (PFN_vkCreateDevice)get(instance, "vkCreateDevice");
  PFN_vkGetDeviceProcAddr device_get =
      (PFN_vkGetDeviceProcAddr)get(instance, "vkGetDeviceProcAddr");
  if (!destroy || !enumerate || !properties || !queues || !memory ||
      !device_create || !device_get)
    return result(e, "FAIL:required Vulkan entrypoint; instance retained");
  char out[512];
  snprintf(out, sizeof(out), "instance created");
  uint32_t count = 0;
  rc = enumerate(instance, &count, NULL);
  VkPhysicalDevice physical = VK_NULL_HANDLE;
  if (mode >= 2) {
    uint32_t capacity = 1;
    if (rc == VK_SUCCESS && count > 0)
      rc = enumerate(instance, &capacity, &physical);
    if ((rc != VK_SUCCESS && rc != VK_INCOMPLETE) || !physical) {
      destroy(instance, NULL);
      return result(e, "UNSUPPORTED:no Vulkan physical device");
    }
  }
  if (mode == 2) {
    VkPhysicalDeviceProperties p;
    properties(physical, &p);
    snprintf(out, sizeof(out), "gpu=%s vendor=%04x device=%04x api=%u",
             p.deviceName, p.vendorID, p.deviceID, p.apiVersion);
  }
  if (mode == 3 || mode == 4) {
    uint32_t n = 0;
    queues(physical, &n, NULL);
    VkQueueFamilyProperties list[64];
    uint32_t capacity = n < 64 ? n : 64;
    queues(physical, &capacity, list);
    uint32_t selected = UINT32_MAX;
    for (uint32_t i = 0; i < capacity; i++)
      if (list[i].queueCount && (list[i].queueFlags & VK_QUEUE_GRAPHICS_BIT)) {
        selected = i;
        break;
      }
    snprintf(out, sizeof(out), "queueFamilies=%u graphics=%u", n, selected);
    if (mode == 4) {
      if (selected == UINT32_MAX)
        snprintf(out, sizeof(out), "UNSUPPORTED:no graphics queue");
      else {
        float priority = 1;
        VkDeviceQueueCreateInfo q = {
            .sType = VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO,
            .queueFamilyIndex = selected,
            .queueCount = 1,
            .pQueuePriorities = &priority};
        VkDeviceCreateInfo d = {.sType = VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO,
                                .queueCreateInfoCount = 1,
                                .pQueueCreateInfos = &q};
        VkDevice device;
        rc = device_create(physical, &d, NULL, &device);
        if (rc == VK_SUCCESS) {
          PFN_vkDestroyDevice del =
              (PFN_vkDestroyDevice)device_get(device, "vkDestroyDevice");
          if (del) {
            del(device, NULL);
            snprintf(out, sizeof(out),
                     "logical device created and destroyed; no queue work "
                     "submitted");
          } else {
            snprintf(out, sizeof(out),
                     "FAIL:vkDestroyDevice missing; resources retained");
            return result(e, out);
          }
        } else
          snprintf(out, sizeof(out), "FAIL:vkCreateDevice result=%d", rc);
      }
    }
  }
  if (mode == 5) {
    VkPhysicalDeviceMemoryProperties m;
    memory(physical, &m);
    unsigned long long bytes = 0;
    for (uint32_t i = 0; i < m.memoryHeapCount; i++)
      bytes += m.memoryHeaps[i].size;
    snprintf(out, sizeof(out), "heaps=%u types=%u advertisedBytes=%llu",
             m.memoryHeapCount, m.memoryTypeCount, bytes);
  }
  destroy(instance, NULL);
  return result(e, out);
}

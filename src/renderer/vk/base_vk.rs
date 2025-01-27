use super::pointer_chain_helpers::*;
use super::vk_debug_callback;
use ash::{extensions::*, vk};
use gpu_allocator::{vulkan as vkalloc, MemoryLocation};
use std::borrow::Borrow;
use std::collections::HashSet;
use std::ffi::{CStr, CString};
use std::mem::ManuallyDrop;

use raw_window_handle::RawWindowHandle;

pub struct BaseVk {
    entry_fn: ash::Entry,
    pub instance: ash::Instance,
    surface: vk::SurfaceKHR,
    surface_fn: Option<khr::Surface>,
    physical_device: vk::PhysicalDevice,
    queue_family_index: u32,
    pub device: ash::Device,
    pub queues: Vec<vk::Queue>,
    pub swapchain_fn: Option<khr::Swapchain>,
    pub swapchain_create_info: Option<vk::SwapchainCreateInfoKHR>,
    pub swapchain: vk::SwapchainKHR,
    pub swapchain_image_views: Option<Vec<vk::ImageView>>,
    pub allocator: ManuallyDrop<gpu_allocator::vulkan::Allocator>,
    #[cfg(debug_assertions)]
    debug_utils_fn: ext::DebugUtils,
    #[cfg(debug_assertions)]
    debug_utils_messenger: vk::DebugUtilsMessengerEXT,
}

#[derive(Clone)]
pub struct BufferAllocation {
    pub buffer: vk::Buffer,
    pub allocation: vkalloc::Allocation,
}

#[derive(Clone)]
pub struct ImageAllocation {
    pub image: vk::Image,
    pub allocation: vkalloc::Allocation,
}

#[derive(Clone)]
pub struct CommandRecordInfo {
    pub pool: vk::CommandPool,
    pub buffers: Vec<vk::CommandBuffer>,
}

#[derive(Clone)]
pub struct DescriptorInfo {
    pub pool: vk::DescriptorPool,
    pub buffers: Vec<vk::DescriptorSet>,
}

/**
BaseVk is struct that initializes a single Vulkan 1.1 instance and device with optional surface support.
It supports instance creation with extensions and device selection with Vulkan 1.1 features
and requested queues. It also initializes an allocator that greatly simplifies Vulkan allocations.
Basically it is a bootstrap for a very common vulkan setup.
*/
impl BaseVk {
    pub fn new(
        application_name: &str,
        instance_extensions: &[&str],
        device_extensions: &[&str],
        desired_physical_device_features2: &vk::PhysicalDeviceFeatures2,
        desired_queues: &[(vk::QueueFlags, f32)],
        window_handle: Option<RawWindowHandle>,
    ) -> Self {
        let application_name = CString::new(application_name).unwrap();
        let application_info = vk::ApplicationInfo::builder()
            .application_name(application_name.as_c_str())
            .application_version(vk::make_api_version(0, 0, 1, 0))
            .engine_name(CStr::from_bytes_with_nul(b"TheVulkanTemple\0").unwrap())
            .engine_version(vk::make_api_version(0, 0, 1, 0))
            .api_version(vk::API_VERSION_1_1);

        let mut instance_extensions: Vec<CString> = instance_extensions
            .iter()
            .map(|s| CString::new(*s).unwrap())
            .collect();

        cfg_if::cfg_if! {
            if #[cfg(debug_assertions)] {
                instance_extensions.push(CString::new("VK_EXT_debug_utils").unwrap());
                let validation_layer_name = CStr::from_bytes_with_nul(b"VK_LAYER_KHRONOS_validation\0")
                    .unwrap()
                    .as_ptr();
                let layer_names = [validation_layer_name];
            } else {
                let layer_names = [];
            }
        }

        // adding the required extensions needed for creating a surface based on the os
        if let Some(handle) = window_handle {
            instance_extensions.push(CString::new("VK_KHR_surface").unwrap());
            match handle {
                RawWindowHandle::Win32(_) => {
                    instance_extensions.push(CString::new("VK_KHR_win32_surface").unwrap());
                }
                RawWindowHandle::Xlib(_) => {
                    instance_extensions.push(CString::new("VK_KHR_xlib_surface").unwrap());
                }
                RawWindowHandle::Wayland(_) => {
                    instance_extensions.push(CString::new("VK_KHR_wayland_surface").unwrap());
                }
                _ => {
                    panic!("Unrecognized window handle")
                }
            };
        }
        let instance_extensions_ptrs = instance_extensions
            .iter()
            .map(|s| s.as_ptr())
            .collect::<Vec<_>>();

        let instance_create_info = vk::InstanceCreateInfo::builder()
            .application_info(&application_info)
            .enabled_layer_names(&layer_names)
            .enabled_extension_names(&instance_extensions_ptrs);

        let entry_fn = unsafe { ash::Entry::load().unwrap() };
        let instance = unsafe {
            entry_fn
                .create_instance(&instance_create_info, None)
                .expect("Could not create VkInstance")
        };

        // Creation of an optional debug reporter
        cfg_if::cfg_if! {
            if #[cfg(debug_assertions)] {
                let debug_utils_messenger_create_info = vk::DebugUtilsMessengerCreateInfoEXT::builder()
                    .message_severity(
                        vk::DebugUtilsMessageSeverityFlagsEXT::INFO
                            | vk::DebugUtilsMessageSeverityFlagsEXT::WARNING
                            | vk::DebugUtilsMessageSeverityFlagsEXT::ERROR
                            | vk::DebugUtilsMessageSeverityFlagsEXT::VERBOSE,
                    )
                    .message_type(
                        vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
                            | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE,
                    )
                    .pfn_user_callback(Some(vk_debug_callback));
                let debug_utils_fn = ext::DebugUtils::new(&entry_fn, &instance);
                let debug_utils_messenger = unsafe {
                    debug_utils_fn
                        .create_debug_utils_messenger(&debug_utils_messenger_create_info, None)
                        .unwrap()
                };
            }
        }

        // Creating the surface based on os
        let surface = unsafe {
            match window_handle {
                Some(RawWindowHandle::Win32(handle)) => {
                    let surface_desc = vk::Win32SurfaceCreateInfoKHR::builder()
                        .hinstance(handle.hinstance)
                        .hwnd(handle.hwnd);
                    let win_surface_fn = khr::Win32Surface::new(&entry_fn, &instance);
                    win_surface_fn
                        .create_win32_surface(&surface_desc, None)
                        .unwrap()
                }
                Some(RawWindowHandle::Xlib(handle)) => {
                    let surface_desc = vk::XlibSurfaceCreateInfoKHR::builder()
                        .dpy(handle.display as *mut _)
                        .window(handle.window);
                    let xlib_surface_fn = khr::XlibSurface::new(&entry_fn, &instance);
                    xlib_surface_fn
                        .create_xlib_surface(&surface_desc, None)
                        .unwrap()
                }
                Some(RawWindowHandle::Wayland(handle)) => {
                    let surface_desc = vk::WaylandSurfaceCreateInfoKHR::builder()
                        .display(handle.display)
                        .surface(handle.surface);
                    let wayland_surface_fn = khr::WaylandSurface::new(&entry_fn, &instance);
                    wayland_surface_fn
                        .create_wayland_surface(&surface_desc, None)
                        .unwrap()
                }
                None => vk::SurfaceKHR::null(),
                _ => panic!("Unsupported window handle"),
            }
        };

        let mut desired_device_extensions: Vec<CString> = device_extensions
            .iter()
            .map(|s| CString::new(*s).unwrap())
            .collect();
        let mut surface_fn = None;
        if surface != vk::SurfaceKHR::null() {
            surface_fn = Some(khr::Surface::new(&entry_fn, &instance));
            desired_device_extensions.push(CString::new("VK_KHR_swapchain").unwrap());
        }

        // Creating a new struct pointer chain to accommodate the features of the physical devices
        let mut available_device_features = unsafe {
            clone_vk_physical_device_features2_structure(desired_physical_device_features2)
        };

        // Iterate for all physical devices and keep only those that respect our requirements
        let good_devices;
        unsafe {
            good_devices = instance
                .enumerate_physical_devices()
                .unwrap()
                .iter()
                .filter_map(|physical_device| {
                    // Check if the physical device supports the required extensions
                    let extensions = instance
                        .enumerate_device_extension_properties(*physical_device)
                        .unwrap();
                    let extensions_names: HashSet<&CStr> = extensions
                        .iter()
                        .map(|v| CStr::from_ptr(v.extension_name.as_ptr()))
                        .collect();
                    if !desired_device_extensions
                        .iter()
                        .all(|e| extensions_names.contains(e.as_c_str()))
                    {
                        return None;
                    }

                    // Check if the physical device supports the features requested
                    instance.get_physical_device_features2(
                        *physical_device,
                        &mut available_device_features,
                    );
                    available_device_features.features.robust_buffer_access = 300;
                    if !compare_vk_physical_device_features2(
                        &available_device_features,
                        desired_physical_device_features2,
                    ) {
                        return None;
                    }

                    // Check if the physical device supports the requested queues
                    let mut queue_family_properties = Vec::<vk::QueueFamilyProperties2>::new();
                    queue_family_properties.resize(
                        instance.get_physical_device_queue_family_properties2_len(*physical_device),
                        vk::QueueFamilyProperties2::default(),
                    );
                    instance.get_physical_device_queue_family_properties2(
                        *physical_device,
                        &mut queue_family_properties,
                    );
                    let good_family_queues =
                        queue_family_properties
                            .iter()
                            .enumerate()
                            .find(|(i, queue_family)| {
                                let mut is_family_queue_good = desired_queues.iter().all(|q| {
                                    queue_family
                                        .queue_family_properties
                                        .queue_flags
                                        .contains(q.0)
                                });
                                is_family_queue_good = is_family_queue_good
                                    && desired_queues.len()
                                        <= queue_family.queue_family_properties.queue_count
                                            as usize;

                                if surface != vk::SurfaceKHR::null() {
                                    is_family_queue_good = is_family_queue_good
                                        && surface_fn
                                            .as_ref()
                                            .unwrap()
                                            .get_physical_device_surface_support(
                                                *physical_device,
                                                *i as u32,
                                                surface,
                                            )
                                            .unwrap();
                                }
                                is_family_queue_good
                            });

                    if let Some(selected_family_queue) = good_family_queues {
                        return Some((*physical_device, selected_family_queue.0 as u32));
                    }
                    None
                })
                .collect::<Vec<(vk::PhysicalDevice, u32)>>();
            destroy_vk_physical_device_features2(&mut available_device_features);
        }

        if good_devices.len() > 1 {
            println!("More than one device available selecting the first");
        }
        // Always selecting the first available device might not be the best strategy
        let selected_device = good_devices.first().expect("No available device found");

        // Device creation
        let device;
        unsafe {
            let queue_priorities = desired_queues.iter().map(|q| q.1).collect::<Vec<f32>>();
            let queues_create_info = vk::DeviceQueueCreateInfo::builder()
                .queue_family_index(selected_device.1)
                .queue_priorities(&queue_priorities)
                .build();
            let device_extensions_ptrs = desired_device_extensions
                .iter()
                .map(|s| s.as_ptr())
                .collect::<Vec<_>>();
            let mut device_create_info = vk::DeviceCreateInfo::builder()
                .queue_create_infos(std::slice::from_ref(&queues_create_info))
                .enabled_extension_names(&device_extensions_ptrs)
                .enabled_features(&desired_physical_device_features2.features);
            device_create_info.p_next = desired_physical_device_features2.p_next;

            device = instance
                .create_device(selected_device.0, &device_create_info, None)
                .expect("Error creating device");
        }

        let mut swapchain_fn = None;
        if window_handle.is_some() {
            swapchain_fn = Some(khr::Swapchain::new(&instance, &device));
        }

        let mut queues = Vec::new();
        for i in 0..desired_queues.len() as u32 {
            queues.push(unsafe { device.get_device_queue(selected_device.1, i) });
        }

        let allocator =
            gpu_allocator::vulkan::Allocator::new(&gpu_allocator::vulkan::AllocatorCreateDesc {
                instance: instance.clone(),
                device: device.clone(),
                physical_device: selected_device.0,
                debug_settings: Default::default(),
                buffer_device_address: false,
            })
            .expect("Could not create Allocator");

        BaseVk {
            entry_fn,
            instance,
            surface,
            surface_fn,
            physical_device: selected_device.0,
            queue_family_index: selected_device.1,
            device,
            queues,
            swapchain_fn,
            swapchain_create_info: None,
            swapchain: vk::SwapchainKHR::null(),
            swapchain_image_views: None,
            allocator: ManuallyDrop::new(allocator),
            #[cfg(debug_assertions)]
            debug_utils_fn,
            #[cfg(debug_assertions)]
            debug_utils_messenger,
        }
    }

    pub fn recreate_swapchain(
        &mut self,
        present_mode: vk::PresentModeKHR,
        window_size: vk::Extent2D,
        usage_flags: vk::ImageUsageFlags,
        surface_format: vk::SurfaceFormatKHR,
    ) {
        self.swapchain_create_info = Some(
            vk::SwapchainCreateInfoKHR::builder()
                .image_array_layers(1)
                .surface(self.surface)
                .pre_transform(vk::SurfaceTransformFlagsKHR::IDENTITY)
                .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
                .clipped(true)
                .old_swapchain(self.swapchain)
                .build(),
        );
        let swapchain_create_info_ref = self.swapchain_create_info.as_mut().unwrap();
        let surface_capabilities;
        unsafe {
            // getting the present mode for the swapchain
            swapchain_create_info_ref.present_mode = *self
                .surface_fn
                .as_ref()
                .expect("BaseVk has not been created with surface support")
                .borrow()
                .get_physical_device_surface_present_modes(self.physical_device, self.surface)
                .unwrap()
                .iter()
                .find(|m| **m == present_mode)
                .unwrap_or(&vk::PresentModeKHR::FIFO);

            surface_capabilities = self
                .surface_fn
                .as_ref()
                .unwrap()
                .get_physical_device_surface_capabilities(self.physical_device, self.surface)
                .unwrap();
        }

        // getting the image count for the swapchain
        swapchain_create_info_ref.min_image_count = surface_capabilities.min_image_count + 1;
        if surface_capabilities.max_image_count != 0 {
            swapchain_create_info_ref.min_image_count = std::cmp::min(
                swapchain_create_info_ref.min_image_count,
                surface_capabilities.max_image_count,
            );
        }

        // getting the extent of the images for the swapchain
        if surface_capabilities.current_extent.width == 0xFFFFFFFF
            && surface_capabilities.current_extent.height == 0xFFFFFFFF
        {
            swapchain_create_info_ref.image_extent.width = num::clamp(
                window_size.width,
                surface_capabilities.min_image_extent.width,
                surface_capabilities.max_image_extent.width,
            );

            swapchain_create_info_ref.image_extent.height = num::clamp(
                window_size.height,
                surface_capabilities.min_image_extent.height,
                surface_capabilities.max_image_extent.height,
            );
        } else {
            swapchain_create_info_ref.image_extent = surface_capabilities.current_extent;
        }

        // checking if the usage flags are supported
        if !surface_capabilities
            .supported_usage_flags
            .contains(usage_flags)
        {
            panic!("Unsupported image usage flags")
        }
        swapchain_create_info_ref.image_usage = usage_flags;

        // checking if the surface format is supported or a substitute needs to be selected
        unsafe {
            let supported_formats = self
                .surface_fn
                .as_ref()
                .unwrap()
                .get_physical_device_surface_formats(self.physical_device, self.surface)
                .unwrap();

            let chosen_format = supported_formats
                .iter()
                .find(|e| **e == surface_format)
                .unwrap_or_else(|| supported_formats.first().unwrap());

            swapchain_create_info_ref.image_format = chosen_format.format;
            swapchain_create_info_ref.image_color_space = chosen_format.color_space;

            self.swapchain = self
                .swapchain_fn
                .as_ref()
                .unwrap()
                .create_swapchain(&self.swapchain_create_info.unwrap(), None)
                .expect("Could not create swapchain");

            if let Some(swapchain_image_views) = &mut self.swapchain_image_views {
                swapchain_image_views
                    .iter()
                    .for_each(|siv| self.device.destroy_image_view(*siv, None));
                swapchain_image_views.clear();
            } else {
                self.swapchain_image_views = Some(Vec::new());
            }

            let swapchain_images = self
                .swapchain_fn
                .as_ref()
                .unwrap()
                .get_swapchain_images(self.swapchain)
                .unwrap();
            for swapchain_image in swapchain_images.iter() {
                let image_view_create_info = vk::ImageViewCreateInfo::builder()
                    .image(*swapchain_image)
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(self.swapchain_create_info.unwrap().image_format)
                    .components(vk::ComponentMapping::default())
                    .subresource_range(
                        vk::ImageSubresourceRange::builder()
                            .aspect_mask(vk::ImageAspectFlags::COLOR)
                            .base_mip_level(0)
                            .level_count(1)
                            .base_array_layer(0)
                            .layer_count(1)
                            .build(),
                    );
                self.swapchain_image_views.as_mut().unwrap().push(
                    self.device
                        .create_image_view(&image_view_create_info, None)
                        .unwrap(),
                );
            }
        }
    }

    pub fn allocate_buffer(
        &mut self,
        buffer_create_info: &vk::BufferCreateInfo,
        memory_location: MemoryLocation,
    ) -> BufferAllocation {
        let buffer = unsafe { self.device.create_buffer(buffer_create_info, None) }.unwrap();
        let requirements = unsafe { self.device.get_buffer_memory_requirements(buffer) };

        let allocation = self
            .allocator
            .allocate(&vkalloc::AllocationCreateDesc {
                name: "",
                requirements,
                location: memory_location,
                linear: true, // buffers are always linear
            })
            .unwrap();

        unsafe {
            self.device
                .bind_buffer_memory(buffer, allocation.memory(), allocation.offset())
                .unwrap()
        };
        BufferAllocation { buffer, allocation }
    }

    pub fn destroy_buffer(&mut self, buffer: &BufferAllocation) {
        self.allocator.free(buffer.allocation.clone()).unwrap();
        unsafe { self.device.destroy_buffer(buffer.buffer, None) };
    }

    pub fn create_cmd_pool_and_buffers(
        &mut self,
        pool_flags: vk::CommandPoolCreateFlags,
        cmdb_level: vk::CommandBufferLevel,
        cmdb_count: u32,
    ) -> CommandRecordInfo {
        let command_pool_create_info = vk::CommandPoolCreateInfo::builder()
            .flags(pool_flags)
            .queue_family_index(self.queue_family_index);
        let pool = unsafe {
            self.device
                .create_command_pool(&command_pool_create_info, None)
                .unwrap()
        };

        let command_buffers_allocate_info = vk::CommandBufferAllocateInfo::builder()
            .command_pool(pool)
            .level(cmdb_level)
            .command_buffer_count(cmdb_count);
        let buffers = unsafe {
            self.device
                .allocate_command_buffers(&command_buffers_allocate_info)
                .unwrap()
        };
        CommandRecordInfo { pool, buffers }
    }

    pub fn destroy_cmd_pool_and_buffers(&mut self, cmri: &CommandRecordInfo) {
        unsafe {
            self.device.free_command_buffers(cmri.pool, &cmri.buffers);
            self.device.destroy_command_pool(cmri.pool, None);
        }
    }

    pub fn create_descriptor_pool_and_sets(
        &mut self,
        pool_sizes: &[vk::DescriptorPoolSize],
        sets: &[vk::DescriptorSetLayout],
    ) -> DescriptorInfo {
        let descriptor_pool_create_info = vk::DescriptorPoolCreateInfo::builder()
            .max_sets(sets.len() as u32)
            .pool_sizes(&pool_sizes);
        let descriptor_pool = unsafe {
            self.device
                .create_descriptor_pool(&descriptor_pool_create_info, None)
                .unwrap()
        };
        let descriptor_set_allocate_info = vk::DescriptorSetAllocateInfo::builder()
            .descriptor_pool(descriptor_pool)
            .set_layouts(sets);
        let descriptor_sets = unsafe {
            self.device
                .allocate_descriptor_sets(&descriptor_set_allocate_info)
                .unwrap()
        };
        DescriptorInfo {
            pool: descriptor_pool,
            buffers: descriptor_sets,
        }
    }

    pub fn destroy_descriptor_pool_and_sets(&mut self, di: &DescriptorInfo) {
        unsafe {
            self.device.destroy_descriptor_pool(di.pool, None);
        }
    }

    pub fn create_semaphores(&mut self, count: u32) -> Vec<vk::Semaphore> {
        let semaphore_create_info = vk::SemaphoreCreateInfo::builder();
        (0..count)
            .map(|_| unsafe {
                self.device
                    .create_semaphore(&semaphore_create_info, None)
                    .unwrap()
            })
            .collect()
    }

    pub fn destroy_semaphores(&mut self, semaphores: &Vec<vk::Semaphore>) {
        semaphores
            .iter()
            .for_each(|s| unsafe { self.device.destroy_semaphore(*s, None) });
    }
}

impl Drop for BaseVk {
    fn drop(&mut self) {
        unsafe {
            ManuallyDrop::drop(&mut self.allocator);
            if let Some(swapchain_image_views) = self.swapchain_image_views.as_ref() {
                for swapchain_image_view in swapchain_image_views.iter() {
                    self.device.destroy_image_view(*swapchain_image_view, None);
                }
            }

            if let Some(fp) = self.swapchain_fn.as_ref() {
                fp.destroy_swapchain(self.swapchain, None);
            }
            self.device.destroy_device(None);
            if let Some(fp) = self.surface_fn.as_ref() {
                fp.destroy_surface(self.surface, None);
            }
            #[cfg(debug_assertions)]
            self.debug_utils_fn
                .destroy_debug_utils_messenger(self.debug_utils_messenger, None);
            self.instance.destroy_instance(None);
        }
    }
}

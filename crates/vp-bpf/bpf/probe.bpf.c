// SPDX-License-Identifier: GPL-2.0-only
#include <linux/bpf.h>
#include <bpf/bpf_helpers.h>
#include <bpf/bpf_core_read.h>

/* Minimal CO-RE view. BTF relocates tgid to the running kernel's layout. */
struct task_struct {
    int tgid;
} __attribute__((preserve_access_index));

const volatile __u32 target_pid = 0;

struct {
    __uint(type, BPF_MAP_TYPE_PERCPU_ARRAY);
    __uint(max_entries, 1);
    __type(key, __u32);
    __type(value, __u64);
} vp_hits SEC(".maps");

SEC("uprobe")
int vp_probe(void *ctx)
{
    struct task_struct *task = (void *)bpf_get_current_task_btf();
    if ((__u32)BPF_CORE_READ(task, tgid) != target_pid)
        return 0;

    __u32 key = 0;
    __u64 *hits = bpf_map_lookup_elem(&vp_hits, &key);
    if (hits)
        __sync_fetch_and_add(hits, 1);
    return 0;
}

char LICENSE[] SEC("license") = "GPL";

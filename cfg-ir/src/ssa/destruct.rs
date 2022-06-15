use std::time;

use crate::{def_use::DefUse, function::Function};

pub fn destruct(function: &mut Function) {
    let now = time::Instant::now();
    let mut def_use = DefUse::new(function);
    let def_use_time = now.elapsed();
    println!("-def use: {:?}", def_use_time);

    for (node, phis) in function
        .blocks()
        .iter()
        .map(|(&node, block)| (node, block.phi_instructions.len()))
        .collect::<Vec<_>>()
    {
        for phi_index in (0..phis).rev() {
            let phi = &function.block_mut(node).unwrap().phi_instructions[phi_index];
            let dest = phi.dest;
            for incoming_value in phi.incoming_values.values().cloned().collect::<Vec<_>>() {
                let incoming_value_def_use = def_use.get(incoming_value).unwrap().clone();
                incoming_value_def_use
                    .reads
                    .into_iter()
                    .chain(incoming_value_def_use.writes.into_iter())
                    .for_each(|incoming_value_use_location| {
                        function
                            .block_mut(incoming_value_use_location.node)
                            .unwrap()
                            .replace_values(
                                incoming_value_use_location.index,
                                incoming_value,
                                dest,
                            );
                        def_use.update_block(
                            function.block(incoming_value_use_location.node).unwrap(),
                            incoming_value_use_location.node,
                        );
                    });
            }
            function
                .block_mut(node)
                .unwrap()
                .phi_instructions
                .remove(phi_index);
            def_use.update_block_phi(function.block(node).unwrap(), node);
        }
    }
}
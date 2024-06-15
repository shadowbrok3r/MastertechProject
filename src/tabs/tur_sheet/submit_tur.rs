use std::{collections::HashMap, path::PathBuf};

use chrono::DateTime;
use log::info;
use serde_json::Value;

use crate::{app_state::MastertechContext, tabs::tur_sheet::get_ticket::SendRequest};

use super::email_builder::{AsanaTask, TaskAssignee};



impl MastertechContext{
    pub fn submit_tur(&mut self){
        self.spinner = true;

        let cust = &self.ticket_info.customer_name;
        let so_num = &self.so_number;

        if !cust.is_empty() && !so_num.is_empty()
        {

            let mut salesman_map = HashMap::new();
            let mut tech_map = HashMap::new();

            let salesman = &self.salesman; // &format!("{:?}", &self.salesman_cbox);
            let checkin_rep = &self.ticket_info.checkin_rep;
            let technician = &self.technician; // &format!("{:?}", &self.techs_cbox);

            salesman_map.insert("Jake", "1202792432658520");
            salesman_map.insert("Danny", "1202791016369879");
            tech_map.insert("Logan", "1199992640930465");
            tech_map.insert("Bread", "1202792432421640");
            tech_map.insert("Taco", "1202792432551073");

            // let assigned_salesman = salesman_map.get(salesman.as_str()).unwrap_or(&"1202792432658520").to_string();
            // let assigned_tech = tech_map.get(technician.as_str()).unwrap_or(&"1199992640930465").to_string();

            let hdd_test = &format!("{:?}", &self.hdd_test_cbox);
            let ram_test = &format!("{:?}", &self.ram_test_cbox);
            let ssd_test = &format!("{:?}", &self.ssd_test_cbox);

            let checkin_notes = &self.ticket_info.checkin_notes;
            let recommendations = &self.recommendations;   

            let date = self.date.unwrap_or(DateTime::default());
            let mut _attached_file: Option<PathBuf> = None;
            if let Some(file) = &self.opened_file{
                _attached_file = Some(file.to_path_buf());
            }

            let mut _specs = String::new();
            let cps = self.current_antivirus.clone();
            let seb_info = self.seb_info.clone().unwrap_or_default();

            

            let mut final_disk = String::new();
            let mut each_disk = String::new();
                                                    
            let cust_code = &self.ticket_info.cust_code;
            let doc_alias = &self.ticket_info.doc_alias;
            let _department = &self.ticket_info.dep;
            //let juris = &self.ticket_info.juris;
            let ticket_total = &self.ticket_info.ticket_total;
            let cust_email = &self.ticket_info.customer_email;
            let last_inv_num = &self.ticket_info.last_invoice_number;
            let last_inv_amt = &self.ticket_info.last_invoice_amount;
            let total_inv_num = &self.ticket_info.total_invoice_count;
            let phone1 = &self.ticket_info.customer_phone_1;
            let phone2 = &self.ticket_info.customer_phone_2;
            let mut phone_2 = String::new();
            if !phone2.is_empty(){
                phone_2 = format!("<tr>
                <td style=\"padding:1px 1px\">Phone #2</td>
                <td colspan=\"2\" data-cell-widths=\"150,150\" style=\"padding:1px 1px\">{phone2}</td>
                </tr>");
            }

            let extra_customer_info = format!
            ("
            <tr>
                <td style=\"padding:1px 1px\">Customer Code</td>
                <td colspan=\"2\" data-cell-widths=\"150,150\" style=\"padding:1px 1px\">{cust_code}</td>
            </tr>
            <tr>
                <td style=\"padding:1px 1px\">Phone #</td>
                <td colspan=\"2\" data-cell-widths=\"150,150\" style=\"padding:1px 1px\"><strong>{phone1}</strong></td>
            </tr>
            {phone_2}
            <tr>
                <td style=\"padding:1px 1px\">Customer Email</td>
                <td colspan=\"2\" data-cell-widths=\"150,150\" style=\"padding:1px 1px\">{cust_email}</td>
            </tr>
            <tr>
                <td style=\"padding:1px 1px\">Current Total</td>
                <td colspan=\"2\" data-cell-widths=\"150,150\" style=\"padding:1px 1px\">${ticket_total}</td>
            </tr>
            <tr>
                <td style=\"padding:1px 1px\">Last SI#</td>
                <td colspan=\"2\" data-cell-widths=\"150,150\" style=\"padding:1px 1px\">{last_inv_num}</td>
            </tr>
            <tr>
                <td style=\"padding:1px 1px\">Last Invoice Total</td>
                <td colspan=\"2\" data-cell-widths=\"150,150\" style=\"padding:1px 1px\">{last_inv_amt}</td>
            </tr>
            <tr>
                <td style=\"padding:1px 1px\"># of SI's</td>
                <td colspan=\"2\" data-cell-widths=\"150,150\" style=\"padding:1px 1px\">{total_inv_num}</td>
            </tr>
            ");
            if self.send_specs == true{
                self.output_text.clear();
                self.output_text += "pulling system information. Please wait a moment..\n";
                let system_name = &self.system_info.hostname;
                let os = &self.system_info.operating_system;
                let cpu_name = &self.system_info.cpu;
                let total_ram = &self.system_info.ram;
                let gpu = &self.system_info.gpu.clone();

                for index in 0..self.disk_num
                {
                    if let Some(disk) = self.disks.get(index)
                    {
                        let drive_letter = format!("{}", disk.get("drive_letter").and_then(Value::as_str).unwrap_or(""));
                        let drive_type = disk.get("drive_type").and_then(Value::as_str).unwrap_or("");
                        let space_left = format!("{} Gb", disk.get("space_left").and_then(Value::as_str).unwrap_or(""));
                        let total_size = format!("{} Gb", disk.get("total_size").and_then(Value::as_str).unwrap_or(""));

                        each_disk += &format!("
                        <tr>
                        <td style=\"padding:1px 1px\">        {drive_letter}</td>
                        <td style=\"padding:1px 1px\">        {drive_type}</td>
                        <td style=\"padding:1px 1px\">        {space_left}</td>
                        <td style=\"padding:1px 1px\">        {total_size}</td>
                        </tr>
                        ");

                        final_disk = format!
                            ("
                            <tr>
                                <td style=\"padding:1px 4px\">Letter</td>
                                <td style=\"padding:1px 4px\">Drive Type</td>
                                <td style=\"padding:1px 4px\">Avail Space</td>
                                <td style=\"padding:1px 4px\">Total Space</td>
                            </tr>
                            {each_disk}
                        ");

                    }
                }

                _specs = format!("
                <table>
                    <tr>
                        <td style=\"text-align:center;\" colspan=\"3\" data-cell-widths=\"130,200,200\" width=\"450\"
                        >              <code>       Computer Info        </code></td>
                    </tr>
                    <tr>
                        <td>PC Name</td>
                        <td colspan=\"2\" data-cell-widths=\"150,150\">{system_name}</td>
                    </tr>
                    <tr>
                        <td>OS</td>
                        <td colspan=\"2\" data-cell-widths=\"150,150\">{os}</td>
                    </tr>
                    <tr>
                        <td>CPU</td>
                        <td colspan=\"2\" data-cell-widths=\"150,150\">{cpu_name}</td>
                    </tr>
                    <tr>
                        <td>RAM</td>
                        <td colspan=\"2\" data-cell-widths=\"150,150\">{total_ram} Gb</td>
                    </tr>
                    <tr>
                        <td>GPU</td>
                        <td colspan=\"2\" data-cell-widths=\"150,150\">{gpu}</td>
                    </tr>
                    <tr>
                        <td>Antivirus</td>
                        <td colspan=\"2\" data-cell-widths=\"150,150\">{cps}</td>
                    </tr>
                    <tr>
                        <td>SEB</td>
                        <td colspan=\"2\" data-cell-widths=\"150,150\">{seb_info:#?}</td>
                    </tr>
                    <tr>
                    <td colspan=
                    \"3\" data-cell-widths=\"100,200,200\" width=\"400\" style=\"text-align:center;\"
                    >                <code>        HDD/SSD info        </code></td>
                    </tr>
                    {final_disk}
                    </table>
                ").trim().to_string();
            }else{
                _specs = "Computer information was not sent with ticket".to_string();
            }
            
            let html_notes = format!(
                "<body>
                    <table>
                        <tr>
                            <td style=\"text-align:center;\" colspan=\"3\" data-cell-widths=\"130,130,130\" width=\"390\"
                            >                <code>        {doc_alias} Info        </code>
                            </td>
                        </tr>
                        <tr>
                            <td style=\"padding:1px 1px\">Salesman</td>
                            <td style=\"padding:1px 1px\">Checkin Rep</td>
                            <td style=\"padding:1px 1px\">Technician</td>
                        </tr>
                        <tr>
                            <td style=\"padding:1px 4px\">     {salesman}</td>
                            <td style=\"padding:1px 4px\">     {checkin_rep}</td>
                            <td style=\"padding:1px 4px\">     {technician}</td>
                        </tr>
                        <tr>
                            <td style=\"text-align:center;\" colspan=\"3\" data-cell-widths=\"130,130,130\" width=\"390\"
                            >                <code>           Customer           </code>
                            </td>
                        </tr>
                        {extra_customer_info}
                    </table>
                    {_specs}
                    <ul>
                        <li><strong>SSD test:</strong>     {ssd_test}</li>
                        <li><strong>HDD test:</strong>     {hdd_test}</li>
                        <li><strong>RAM test:</strong>     {ram_test}</li>
                    </ul>
                    <h2><strong><code>           Notes           </code></strong></h2>
                    <ul><li><strong>        Checkin Notes:      </strong>     \n{checkin_notes}</li>
                        <li><strong>        Recommendations:        </strong>     \n{recommendations}</li></ul></body>",
            );

            let store = &mut self.ticket_info.dep;

            if store.as_str() == "RIV"{

                let task = AsanaTask { 
                    task_name: format!("{cust} - {so_num}"), 
                    html_notes,
                    assignee: TaskAssignee { 
                        salesman: self.salesman.clone(), 
                        tech: self.technician.clone()
                    }, 
                    file_attachment: self.opened_file.clone() 
                };
                let tx = self.scaffold_request.tx.clone();
                let client = self.client.clone();
                tokio::spawn(async move {
                    let _ = SendRequest::send_ticket_request(
                        tx, 
                        client, 
                        task,
                        date,
                    ).await.unwrap();
                    info!("After tokio spawn in send_ticket_request");
                });


            }else{
                self.submit_tur_email();
            }

            self.spinner = false;
            
            self.output_text += "\nSent Ticket";
        }
        else{
            self.output_text.clear();
            self.output_text = "You need to enter a customer name or Service number".to_string();
        }
    

        self.spinner = false;
        self.ctx.request_repaint();

    }
}